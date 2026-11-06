//! Global command overlay. Navigation stays local; mutations go through Source::net.
use crate::{
    App, Appearance, Route, data, icons, kit,
    theme::{self, AlphaMul, T},
};
use rune::{Cx, El, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, el, icon, row, text, text_input};
use unicode_normalization::{UnicodeNormalization, char::is_combining_mark};

#[derive(Default)]
pub struct Ui {
    pub open: bool,
    pub query: TextEdit,
    pub selected: usize,
    pub page: String,
    pub gallery: Option<&'static str>,
    pub gallery_edit: TextEdit,
    pub gallery_on: bool,
    pub gallery_menu: bool,
    pub gallery_combo: kit::ComboboxState,
    searches: Vec<(u64, String)>,
    acknowledgements: Vec<u64>,
    message_query: String,
    search_due: Option<i64>,
    message_matches: Vec<serde_json::Value>,
    restart: Option<Restart>,
    notice: Option<(String, i64)>,
}
struct Restart {
    thread: String,
    remaining: Vec<String>,
    pending: u64,
    refresh: serde_json::Value,
    refreshed: bool,
}
#[derive(Clone)]
enum Action {
    Navigate(Route),
    Page(&'static str),
    Theme(Appearance),
    Restart(String),
    OpenProject(String),
}
struct SearchTerm {
    value: String,
    weight: f32,
    context: bool,
    keyword: bool,
}
impl SearchTerm {
    fn context(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            weight: 0.25,
            context: true,
            keyword: false,
        }
    }
    fn weighted(value: impl Into<String>, weight: f32) -> Self {
        Self {
            value: value.into(),
            weight,
            context: false,
            keyword: false,
        }
    }
    fn keyword(value: impl Into<String>) -> Self {
        Self {
            value: value.into(),
            weight: 0.6,
            context: false,
            keyword: true,
        }
    }
}
struct Item {
    title: String,
    group: &'static str,
    glyph: &'static rune::Icon,
    shortcut: &'static str,
    action: Action,
    subtitle: Option<String>,
    timestamp: Option<String>,
    fields: Vec<SearchTerm>,
    pick: (i64, bool),
    references: Vec<String>,
    usage: f32,
}
fn item(
    title: impl Into<String>,
    group: &'static str,
    glyph: &'static rune::Icon,
    shortcut: &'static str,
    action: Action,
) -> Item {
    Item {
        title: title.into(),
        group,
        glyph,
        shortcut,
        action: action.clone(),
        subtitle: None,
        timestamp: None,
        pick: (0, false),
        references: vec![],
        fields: match &action {
            Action::Theme(_) => vec![],
            Action::Restart(_) => [
                "restart", "reset", "reload", "agent", "session", "skills", "plugins", "mcp",
            ]
            .into_iter()
            .map(SearchTerm::keyword)
            .collect(),
            Action::Page("projects") => ["new thread", "project", "pick", "choose", "select"]
                .into_iter()
                .map(SearchTerm::keyword)
                .collect(),
            Action::Navigate(Route::Settings(_)) => {
                ["settings", "preferences", "configuration", "keybindings"]
                    .into_iter()
                    .map(SearchTerm::keyword)
                    .collect()
            }
            Action::Navigate(Route::Home { .. }) => ["new thread", "chat", "create", "draft"]
                .into_iter()
                .map(SearchTerm::keyword)
                .collect(),
            _ => vec![],
        },
        usage: 1.,
    }
}

fn items(s: &App) -> Vec<Item> {
    let ui = &s.ui.shell.palette;
    let mut out = vec![];
    if ui.page == "themes" {
        out.extend(
            [
                ("System", Appearance::System),
                ("Light", Appearance::Light),
                ("Dark", Appearance::Dark),
            ]
            .into_iter()
            .map(|(label, mode)| {
                item(
                    label,
                    "Change theme",
                    &icons::PALETTE,
                    "",
                    Action::Theme(mode),
                )
            }),
        );
    }
    if ui.page.is_empty() {
        let project = match &s.route {
            Route::Thread(id) => s
                .state()
                .threads
                .iter()
                .find(|v| data::s(v, "id") == id)
                .map(|v| data::s(v, "projectId")),
            Route::Home { project_id } => project_id.as_deref(),
            _ => None,
        };
        if let Some(p) =
            project.and_then(|id| s.state().projects.iter().find(|v| data::s(v, "id") == id))
        {
            out.push(item(
                format!("New thread in {}", data::s(p, "title")),
                "Actions",
                &icons::SQUARE_PEN,
                "⇧⌘N",
                Action::Navigate(Route::Home {
                    project_id: Some(data::s(p, "id").into()),
                }),
            ));
        }
        out.push(item(
            "New thread in...",
            "Actions",
            &icons::SQUARE_PEN,
            "",
            Action::Page("projects"),
        ));
        out.push(item(
            "New thread without a project",
            "Actions",
            &icons::MESSAGE_SQUARE_DASHED,
            "",
            Action::Navigate(Route::Home { project_id: None }),
        ));
        if let Route::Thread(id) = &s.route {
            out.push(item(
                "Restart agent session",
                "Actions",
                &icons::ROTATE_CCW,
                "",
                Action::Restart(id.clone()),
            ));
        }
        out.push(item(
            "Open settings",
            "Actions",
            &icons::SETTINGS,
            "⌘,",
            Action::Navigate(Route::Settings("general".into())),
        ));
        out.push(item(
            "Change theme",
            "Actions",
            &icons::PALETTE,
            "",
            Action::Page("themes"),
        ));
        let mut threads: Vec<_> = s
            .state()
            .threads
            .iter()
            .filter(|v| v["archivedAt"].is_null())
            .collect();
        threads.sort_by_key(|v| std::cmp::Reverse(recent_use(v)));

        let limit = if ui.query.text.trim().is_empty() {
            20
        } else {
            threads.len()
        };
        for v in threads.into_iter().take(limit) {
            out.push(item(
                data::s(v, "title"),
                if ui.query.text.trim().is_empty() {
                    "Recent Threads"
                } else {
                    "Threads"
                },
                &icons::MESSAGE_SQUARE,
                "",
                Action::Navigate(Route::Thread(data::s(v, "id").into())),
            ));
            if let Some(row) = out.last_mut() {
                let project = s
                    .state()
                    .projects
                    .iter()
                    .find(|p| data::s(p, "id") == data::s(v, "projectId"))
                    .map(|p| data::s(p, "title"))
                    .unwrap_or("");
                row.subtitle = Some(format!("{project} · Local · ∞"));
                row.fields.push(SearchTerm::weighted(
                    data::s(v, "branch")
                        .trim_start_matches("t3/")
                        .trim_start_matches("t3code/"),
                    0.6,
                ));
                row.fields.push(SearchTerm::context(project));
                row.references.push(data::s(v, "id").into());
                for pr in v["pullRequests"].as_array().into_iter().flatten() {
                    row.fields
                        .push(SearchTerm::weighted(format!("#{}", pr["number"]), 1.));
                    row.fields
                        .push(SearchTerm::weighted(data::s(pr, "title"), 0.5));
                    row.references.push(data::s(pr, "url").into());
                }
                row.pick = active_pick(v, &normalize_query(&ui.query.text), s.now_ms);
                row.usage = usage_boost(v, s.now_ms);

                let minutes =
                    ((s.now_ms - data::ts(v, "updatedAt").unwrap_or(s.now_ms)) / 60_000).max(0);
                row.timestamp = Some(if minutes == 0 {
                    "now".into()
                } else if minutes < 60 {
                    format!("{minutes}m ago")
                } else if minutes < 1440 {
                    format!("{}h ago", minutes / 60)
                } else {
                    format!("{}d ago", minutes / 1440)
                });
            }
        }
    }
    if ui.page == "projects" || (!ui.query.text.trim().is_empty() && ui.page.is_empty()) {
        for p in &s.state().projects {
            out.push(item(
                data::s(p, "title"),
                "Projects",
                &icons::FOLDER_GIT_2,
                "",
                if ui.page == "projects" {
                    Action::Navigate(Route::Home {
                        project_id: Some(data::s(p, "id").into()),
                    })
                } else {
                    Action::OpenProject(data::s(p, "id").into())
                },
            ));
        }
    }
    if !ui.query.text.trim().is_empty() && ui.page.is_empty() {
        for (label, page) in [
            ("General", "general"),
            ("Appearance", "appearance"),
            ("Keybindings", "keybindings"),
            ("Providers", "providers"),
            ("Connections", "connections"),
        ] {
            out.push(item(
                label,
                "Settings",
                &icons::SETTINGS,
                "",
                Action::Navigate(Route::Settings(page.into())),
            ));
        }
    }
    let query = normalize_query(&ui.query.text);
    if !query.is_empty() {
        let rank = |v: &Item, typos: bool| -> Option<(f32, bool, f32)> {
            let thread = matches!(v.action, Action::Navigate(Route::Thread(_)));
            let reference = reference_match(&v.references, &query);
            let normalized = if thread {
                thread_query(&query)
            } else {
                query.clone()
            };
            let query = normalized.as_str();
            let meaningful: Vec<_> = query.split_whitespace().filter(|v| !filler(v)).collect();
            let short = meaningful.join(" ").len() < 4;
            let mut total = 0.;
            let mut count = 0;
            let mut strong = true;
            let mut primary_matches = false;
            let primary = field(&v.title);
            for token in query.split_whitespace() {
                let needle: Vec<_> = token.chars().collect();
                let primary_score = match_token(&primary, &needle, typos, true, thread);
                let mut best = primary_score;
                let mut at_word_start = primary_score >= 0.7;
                primary_matches |= primary_score > 0.;
                for extra in &v.fields {
                    let score = match_token(
                        &field(&extra.value),
                        &needle,
                        typos && !extra.keyword,
                        false,
                        false,
                    );
                    if extra.keyword && score < if short { 1. } else { 0.7 } {
                        continue;
                    }
                    if score >= 0.7 && !extra.context {
                        at_word_start = true;
                    }
                    best = best.max(
                        score
                            * if extra.keyword && short {
                                0.8
                            } else {
                                extra.weight
                            },
                    );
                }
                if best == 0. {
                    if !meaningful.is_empty() && filler(token) {
                        continue;
                    }
                    return reference.then_some((1., true, 1.));
                }
                strong &= at_word_start;
                total += best;
                count += 1;
            }
            if count == 0 {
                return None;
            }
            let name = fold(&v.title);
            let quality = total / count as f32
                + if name == query {
                    0.5
                } else if query.contains(' ') && name.contains(query) {
                    0.25
                } else {
                    0.
                };
            let group_quality = if thread {
                quality.min(1.49)
            } else if !primary_matches {
                quality.min(0.65)
            } else {
                quality
            };
            Some((quality, strong, group_quality))
        };
        let typos = !out
            .iter()
            .any(|v| rank(v, false).is_some_and(|(_, strong, _)| strong));
        let mut groups: Vec<(&str, Vec<(Item, f32, f32)>)> = vec![];
        for v in out {
            if let Some((quality, _, group_quality)) = rank(&v, typos) {
                if let Some((_, rows)) = groups.iter_mut().find(|(group, _)| *group == v.group) {
                    rows.push((v, quality, group_quality));
                } else {
                    groups.push((v.group, vec![(v, quality, group_quality)]));
                }
            }
        }
        let class = |q: f32| {
            if q >= 1.5 {
                3
            } else if q >= 0.7 {
                2
            } else if q >= 0.4 {
                1
            } else {
                0
            }
        };
        groups.sort_by_key(|(_, rows)| {
            std::cmp::Reverse(
                rows.iter()
                    .map(|(_, _, group_quality)| class(*group_quality))
                    .max()
                    .unwrap_or(0),
            )
        });
        out = groups
            .into_iter()
            .flat_map(|(_, mut rows)| {
                rows.sort_by(|(a, qa, _), (b, qb, _)| {
                    class(*qb)
                        .min(2)
                        .cmp(&class(*qa).min(2))
                        .then_with(|| {
                            (if b.pick.1 { b.pick.0 } else { 0 })
                                .cmp(&(if a.pick.1 { a.pick.0 } else { 0 }))
                        })
                        .then_with(|| class(*qb).cmp(&class(*qa)))
                        .then_with(|| b.pick.0.cmp(&a.pick.0))
                        .then_with(|| (qb * b.usage).total_cmp(&(qa * a.usage)))
                });
                let limit = if !ui.page.is_empty() {
                    usize::MAX
                } else if rows.first().is_some_and(|(v, _, _)| v.group == "Threads") {
                    20
                } else {
                    3
                };
                rows.into_iter().take(limit).map(|(v, _, _)| v)
            })
            .collect();
        if ui.message_query == query && ui.page.is_empty() {
            let mut hits: Vec<_> = ui.message_matches.iter().collect();
            let subject = |hit: &serde_json::Value| {
                s.state()
                    .threads
                    .iter()
                    .find(|v| v["id"] == hit["threadId"])
            };
            let key = |hit: &serde_json::Value| {
                let quality = hit["quality"].as_f64().unwrap_or(1.) as f32;
                let pick = subject(hit)
                    .map(|v| active_pick(v, &query, s.now_ms).0)
                    .unwrap_or(0);
                let usage = subject(hit).map(|v| usage_boost(v, s.now_ms)).unwrap_or(1.);
                let score = (1. + (1. + hit["hits"].as_f64().unwrap_or(1.) as f32).log2()) * usage;
                (quality, pick, score)
            };
            hits.sort_by(|a, b| {
                let (qa, pa, sa) = key(a);
                let (qb, pb, sb) = key(b);
                (qb >= 0.8)
                    .cmp(&(qa >= 0.8))
                    .then_with(|| pb.cmp(&pa))
                    .then_with(|| qb.total_cmp(&qa))
                    .then_with(|| sb.total_cmp(&sa))
                    .then_with(|| {
                        data::s(b, "messageCreatedAt").cmp(data::s(a, "messageCreatedAt"))
                    })
            });
            for hit in hits.into_iter().take(10) {
                let id = data::s(hit, "threadId");
                if out.iter().any(
                    |v| matches!(&v.action,Action::Navigate(Route::Thread(other)) if other == id),
                ) {
                    continue;
                }
                let title = s
                    .state()
                    .threads
                    .iter()
                    .find(|v| data::s(v, "id") == id)
                    .map(|v| data::s(v, "title"))
                    .unwrap_or("Thread");
                let mut row = item(
                    title,
                    "Messages",
                    &icons::MESSAGE_SQUARE,
                    "",
                    Action::Navigate(Route::Thread(id.into())),
                );
                row.subtitle = Some(data::s(hit, "snippet").into());
                out.push(row);
            }
        }
    }
    out
}
fn filler(token: &str) -> bool {
    [
        "a", "an", "and", "for", "in", "of", "on", "the", "to", "with",
    ]
    .contains(&token)
}
fn fold(source: &str) -> String {
    source
        .chars()
        .map(|c| {
            let normalized: String = c
                .to_string()
                .nfkd()
                .filter(|c| !is_combining_mark(*c))
                .flat_map(char::to_lowercase)
                .map(|c| match c {
                    'ς' => 'σ',
                    '‘' | '’' => '\'',
                    _ => c,
                })
                .collect();
            if normalized.chars().count() == 1 {
                normalized
            } else {
                c.to_lowercase().collect()
            }
        })
        .collect()
}
fn normalize_query(query: &str) -> String {
    fold(
        &query
            .trim_matches(|c: char| {
                c.is_whitespace() || ['"', '\'', '`', '“', '”', '‘', '’'].contains(&c)
            })
            .nfc()
            .collect::<String>(),
    )
    .split_whitespace()
    .collect::<Vec<_>>()
    .join(" ")
}
fn limit_utf16(query: &str, limit: usize) -> String {
    let mut result = String::new();
    let mut units = 0;
    for c in query.chars() {
        if units + c.len_utf16() > limit {
            break;
        }
        units += c.len_utf16();
        result.push(c);
    }
    result
}
fn thread_query(query: &str) -> String {
    let mut query = query;
    for prefix in ["pull request", "pull", "pr"] {
        if let Some(rest) = query.strip_prefix(prefix) {
            let rest = rest.trim_start().trim_start_matches('#');
            if rest.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                query = rest;
                break;
            }
        }
    }
    query = query.trim_start_matches('#').trim_start();
    for prefix in ["t3/", "t3code/"] {
        if let Some(rest) = query.strip_prefix(prefix)
            && !rest.is_empty()
        {
            query = rest;
            break;
        }
    }
    query.into()
}
fn reference_match(references: &[String], query: &str) -> bool {
    if query.chars().count() < 8 {
        return false;
    }
    let anywhere = query.chars().any(|c| c.is_ascii_digit());
    let not_cut = |s: &str, at: usize| {
        !s.get(at..)
            .and_then(|s| s.chars().next())
            .is_some_and(|c| c.is_ascii_digit())
    };
    references
        .iter()
        .filter(|v| !v.is_empty())
        .any(|reference| {
            let reference = reference.to_lowercase();
            if anywhere {
                reference
                    .find(query)
                    .is_some_and(|at| not_cut(&reference, at + query.len()))
                    || (query.starts_with(&reference) && not_cut(query, reference.len()))
            } else {
                reference.starts_with(query)
            }
        })
}
fn active_pick(thread: &serde_json::Value, query: &str, now: i64) -> (i64, bool) {
    let mut cut = String::new();
    let mut units = 0;
    for c in query.chars() {
        if units + c.len_utf16() > 32 {
            break;
        }
        units += c.len_utf16();
        cut.push(c);
    }
    let query = cut.trim();
    let prefix = query.encode_utf16().count() >= 3;
    let mut latest = (0, false);
    let mut exact = 0;
    for pick in thread["opens"]["picks"].as_array().into_iter().flatten() {
        let at = pick["at"].as_i64().unwrap_or(0);
        let text = data::s(pick, "query");
        if now - at > 21 * 86_400_000 {
            continue;
        }
        if text == query {
            exact = exact.max(at);
        }
        if (text == query || (prefix && text.starts_with(query))) && at > latest.0 {
            latest = (at, false);
        }
    }
    if exact > 0 { (exact, true) } else { latest }
}
fn activity_at(thread: &serde_json::Value) -> Option<i64> {
    if thread.get("latestUserAuthoredMessageAt").is_none() {
        data::ts(thread, "latestUserMessageAt")
            .or_else(|| data::ts(thread, "updatedAt"))
            .or_else(|| data::ts(thread, "createdAt"))
    } else {
        data::ts(thread, "latestUserAuthoredMessageAt").or_else(|| data::ts(thread, "createdAt"))
    }
}
fn usage_boost(thread: &serde_json::Value, now: i64) -> f32 {
    let decay = |at: i64| 2_f32.powf(-((now - at).max(0) as f32) / (14. * 86_400_000.));
    let recency = activity_at(thread).map(decay).unwrap_or(0.);
    let opens = &thread["opens"];
    let open_score = opens["score"].as_f64().unwrap_or(0.) as f32
        * decay(opens["openedAt"].as_i64().unwrap_or(0));
    (1. + recency + 0.5 * (1. + open_score).log2()).min(2.5)
}
fn recent_use(thread: &serde_json::Value) -> i64 {
    activity_at(thread)
        .unwrap_or(0)
        .max(thread["opens"]["openedAt"].as_i64().unwrap_or(0))
}
fn token_score(source: &str, token: &str, typos: bool, mid_word: bool) -> f32 {
    score_options(source, token, typos, mid_word)
        .map(|score| score - if fold(source) == token { 0.5 } else { 0. })
        .unwrap_or(0.)
}

/// Request only queries that can search message content; replies are applied after Net drains.
pub fn search_changed(s: &mut App, tx: &mut Tx) {
    let query = normalize_query(&s.ui.shell.palette.query.text);
    s.ui.shell.palette.selected = 0;
    s.ui.shell.palette.search_due = None;
    s.ui.shell.palette.message_matches.clear();
    s.ui.shell.palette.message_query = query.clone();
    if !s.ui.shell.palette.page.is_empty()
        || !query.split_whitespace().any(|v| {
            v.chars().count() >= 3
                || (v.chars().count() == 2
                    && v.chars().any(
                        |c| matches!(c as u32,0x3040..=0x30ff|0x3400..=0x9fff|0xac00..=0xd7af),
                    ))
        })
    {
        return;
    }
    s.ui.shell.palette.search_due = Some(data::now_ms() + 200);
    tx.wake_after(std::time::Duration::from_millis(200));
}
pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let acknowledgements = std::mem::take(&mut s.ui.shell.palette.acknowledgements);
    for id in acknowledgements {
        if s.src.net().and_then(|net| net.take_reply(id)).is_none() {
            s.ui.shell.palette.acknowledgements.push(id);
        }
    }
    if s.ui
        .shell
        .palette
        .notice
        .as_ref()
        .is_some_and(|(_, until)| data::now_ms() >= *until)
    {
        s.ui.shell.palette.notice = None;
        changed = true;
    }
    if let Some(mut restart) = s.ui.shell.palette.restart.take() {
        match s.src.net().and_then(|net| net.take_reply(restart.pending)) {
            None => s.ui.shell.palette.restart = Some(restart),
            Some(Err(_)) => {
                s.ui.shell.palette.notice = Some((
                    "Unable to restart agent session".into(),
                    data::now_ms() + 4000,
                ));
                changed = true;
            }
            Some(Ok(_)) => {
                if restart.refreshed {
                    s.ui.shell.palette.notice = Some((
                        "Agent session will restart on your next message".into(),
                        data::now_ms() + 4000,
                    ));
                    changed = true;
                } else if let Some(net) = s.src.net() {
                    restart.pending = if let Some(session) = restart.remaining.pop() {
                        net.dispatch(serde_json::json!({"type":"provider-session.detach","threadId":restart.thread,"providerSessionId":session,"reason":"client-requested"}))
                    } else {
                        restart.refreshed = true;
                        net.request("server.refreshProviders", restart.refresh.clone())
                    };
                    s.ui.shell.palette.restart = Some(restart);
                }
            }
        }
    }
    if s.ui
        .shell
        .palette
        .search_due
        .is_some_and(|due| data::now_ms() >= due)
    {
        s.ui.shell.palette.search_due = None;
        let query = s.ui.shell.palette.message_query.clone();
        let raw =
            s.ui.shell
                .palette
                .query
                .text
                .trim_matches(|c: char| {
                    c.is_whitespace() || ['"', '\'', '`', '“', '”', '‘', '’'].contains(&c)
                })
                .to_owned();
        if let Some(net) = s.src.net() {
            let id = net.request(
                "orchestration.searchThreads",
                serde_json::json!({"query":limit_utf16(&raw,200),"limit":10}),
            );
            s.ui.shell.palette.searches.push((id, query));
        }
    }
    let pending = std::mem::take(&mut s.ui.shell.palette.searches);
    for (id, query) in pending {
        let reply = s.src.net().and_then(|net| net.take_reply(id));
        match reply {
            Some(reply) => {
                if query == s.ui.shell.palette.message_query {
                    s.ui.shell.palette.message_matches = reply
                        .ok()
                        .and_then(|v| v["matches"].as_array().cloned())
                        .unwrap_or_default();
                    changed = true;
                }
            }
            None => s.ui.shell.palette.searches.push((id, query)),
        }
    }
    changed
}
/// Word starts, camel-case boundaries, prefixes, joined words and one-edit typo matching.
/// Mirrors the primary-name pass in packages/shared/src/paletteSearch.ts.
fn score(source: &str, query: &str) -> Option<f32> {
    score_options(source, query, true, true)
}
struct SearchField {
    text: Vec<char>,
    starts: Vec<bool>,
}
fn field(source: &str) -> std::rc::Rc<SearchField> {
    use std::{
        cell::RefCell,
        collections::{HashMap, VecDeque},
        rc::Rc,
    };
    struct Cache {
        values: HashMap<String, Rc<SearchField>>,
        order: VecDeque<String>,
    }
    thread_local! {static CACHE:RefCell<Cache>=RefCell::new(Cache {values:HashMap::new(),order:VecDeque::new()});}
    CACHE.with(|cache|{
        let mut cache=cache.borrow_mut();
        if let Some(field)=cache.values.get(source) {return field.clone();}
        let chars:Vec<_>=source.chars().collect();
        let starts=chars.iter().enumerate().map(|(i,c)| {
            let prev=i.checked_sub(1).map(|j|chars[j]);
            let apostrophe=matches!(prev,Some('\''|'’')) && i>=2 && chars[i-2].is_alphanumeric();
            !apostrophe && (c.is_alphanumeric() || !c.is_ascii()) && (i==0 || !chars[i-1].is_alphanumeric()
                || (chars[i-1].is_lowercase() && c.is_uppercase())
                || (i+1<chars.len() && chars[i-1].is_uppercase() && c.is_uppercase() && chars[i+1].is_lowercase())
                || chars[i-1].is_numeric()!=c.is_numeric() || matches!(*c as u32,0x3040..=0x30ff|0x3400..=0x4dbf|0x4e00..=0x9fff|0xf900..=0xfaff))
        }).collect();
        let field=Rc::new(SearchField {text:fold(source).chars().collect(),starts});
        if cache.order.len()>=100_000 && let Some(oldest)=cache.order.pop_front() {cache.values.remove(&oldest);}
        cache.order.push_back(source.into());cache.values.insert(source.into(),field.clone());field
    })
}
fn match_token(
    field: &SearchField,
    token: &[char],
    typos: bool,
    primary: bool,
    mid_word: bool,
) -> f32 {
    if token.is_empty() {
        return 0.;
    }
    let text = &field.text;
    let mut best: f32 = 0.;
    for i in 0..text.len() {
        if !text[i..].starts_with(token) {
            continue;
        }
        if field.starts[i] || !token[0].is_alphanumeric() {
            let end = i + token.len();
            let whole = end == text.len() || field.starts[end] || !text[end].is_alphanumeric();
            let mut run_end = end;
            while run_end < text.len() && text[run_end].is_alphanumeric() {
                run_end += 1;
            }
            best = best.max(if whole {
                1.
            } else {
                0.7 + 0.2 * token.len() as f32 / (run_end - i) as f32
            });
            if best == 1. {
                return best;
            }
        } else if mid_word && token.len() >= 4 {
            best = best.max(0.3);
        }
    }
    if best <= 0.3 && primary && joined_words(text, &field.starts, token) {
        return 0.6;
    }
    if best > 0.3 || !typos || token.len() < 3 || !token.iter().any(|c| c.is_alphabetic()) {
        return best;
    }
    for i in 0..text.len() {
        if !field.starts[i]
            || !(text[i] == token[0]
                || (token.len() > 3 && text[i] == token[1] && text.get(i + 1) == Some(&token[0])))
        {
            continue;
        }
        let mut run_end = i + 1;
        while run_end < text.len() && text[run_end].is_alphanumeric() {
            run_end += 1;
        }
        let short = token.len() <= 4;
        let lengths = if short {
            vec![token.len(), token.len() + 1]
        } else {
            vec![token.len(), token.len() - 1, token.len() + 1]
        };
        for len in lengths {
            if i + len <= run_end && one_edit(token, &text[i..i + len], short && len == token.len())
            {
                return 0.45;
            }
        }
    }
    best
}
fn score_options(source: &str, query: &str, typos: bool, mid_word: bool) -> Option<f32> {
    let prepared = field(source);
    let mut total = 0.;
    let mut count = 0;
    let meaningful = query.split_whitespace().any(|v| !filler(v));
    for token in query.split_whitespace() {
        let best = match_token(
            &prepared,
            &token.chars().collect::<Vec<_>>(),
            typos,
            true,
            mid_word,
        );
        if best == 0. {
            if meaningful && filler(token) {
                continue;
            }
            return None;
        }
        total += best;
        count += 1;
    }
    if count == 0 {
        return None;
    }
    let source = fold(source);
    Some(
        total / count as f32
            + if source == query {
                0.5
            } else if query.contains(' ') && source.contains(query) {
                0.25
            } else {
                0.
            },
    )
}
fn joined_words(field: &[char], starts: &[bool], token: &[char]) -> bool {
    let end = |at: usize| {
        let mut end = at + 1;
        while end < field.len() && !starts[end] && field[end].is_alphanumeric() {
            end += 1;
        }
        end
    };
    for start in 0..field.len() {
        if !starts[start] || token.first() != field.get(start) {
            continue;
        }
        let mut at = start;
        let mut used = 0;
        let mut runs = Vec::new();
        while used < token.len() && at < field.len() {
            let word_end = end(at);
            let length = (token.len() - used).min(word_end - at);
            if field[at..at + length] != token[used..used + length] {
                break;
            }
            runs.push((at, at + length));
            used += length;
            if used == token.len() {
                break;
            }
            at = word_end;
            while at < field.len() && !starts[at] {
                at += 1;
            }
        }
        if used != token.len() || runs.len() < 2 {
            continue;
        }
        let (last_start, last_end) = *runs.last().unwrap();
        if last_end == end(last_start)
            || last_end - last_start >= 3
            || Some(start) == starts.iter().position(|v| *v)
            || runs[..runs.len() - 1].iter().all(|(a, b)| b - a >= 3)
        {
            return true;
        }
    }
    false
}
fn one_edit(a: &[char], b: &[char], swap_only: bool) -> bool {
    if a.len().abs_diff(b.len()) > 1 {
        return false;
    }
    let i = a.iter().zip(b).take_while(|(x, y)| x == y).count();
    let (a, b) = (&a[i..], &b[i..]);
    let swap = a.len() > 1 && b.len() > 1 && a[0] == b[1] && a[1] == b[0] && a[2..] == b[2..];
    swap || (!swap_only
        && (a == b
            || (a.len() > 0 && a[1..] == *b)
            || (b.len() > 0 && *a == b[1..])
            || (a.len() > 0 && b.len() > 0 && a[1..] == b[1..])))
}
fn execute(s: &mut App, action: Action) {
    match action {
        Action::Page(page) => {
            let ui = &mut s.ui.shell.palette;
            ui.search_due = None;
            ui.message_matches.clear();
            ui.page = page.into();
            ui.query = TextEdit::default();
            ui.query.focused = true;
            ui.selected = 0;
            return;
        }
        Action::Navigate(route) => {
            if let Route::Thread(id) = &route {
                let visited = s
                    .state()
                    .threads
                    .iter()
                    .find(|v| v["id"] == *id)
                    .map(|v| data::s(v, "updatedAt").to_owned());
                let query = limit_utf16(&normalize_query(&s.ui.shell.palette.query.text), 200);
                if let Some(visited) = visited
                    && let Some(net) = s.src.net()
                {
                    let ack=net.dispatch(serde_json::json!({"type":"thread.visit","threadId":id,"visitedAt":visited,"opened":true,"searchQuery":query}));
                    s.ui.shell.palette.acknowledgements.push(ack);
                }
            }
            s.navigate(route)
        }
        Action::Theme(mode) => s.appearance = mode,
        Action::OpenProject(project) => {
            let thread = s
                .state()
                .threads
                .iter()
                .filter(|v| {
                    v["projectId"] == project
                        && v["archivedAt"].is_null()
                        && v["lineage"]["parentThreadId"].is_null()
                })
                .max_by_key(|v| recent_use(v))
                .map(|v| data::s(v, "id").to_owned());
            let route = thread.map(Route::Thread).unwrap_or(Route::Home {
                project_id: Some(project),
            });
            execute(s, Action::Navigate(route));
        }
        Action::Restart(id) => {
            if s.ui.shell.palette.restart.is_some() {
                return;
            }
            let sessions = s
                .state()
                .thread_views
                .get(&id)
                .and_then(|v| v.projection["providerSessions"].as_array())
                .into_iter()
                .flatten()
                .filter_map(|v| v["id"].as_str().map(str::to_owned))
                .collect::<Vec<_>>();
            let thread = s.state().threads.iter().find(|v| data::s(v, "id") == id);
            let project = thread.and_then(|v| {
                s.state()
                    .projects
                    .iter()
                    .find(|p| p["id"] == v["projectId"])
            });
            let refresh = serde_json::json!({
                "instanceId":thread.map(|v|v["modelSelection"]["instanceId"].clone()).unwrap_or(serde_json::Value::Null),
                "cwd":thread.and_then(|v|v["worktreePath"].as_str()).or_else(||project.and_then(|v|v["workspaceRoot"].as_str())),
                "fresh":true
            });
            let refresh: serde_json::Value = refresh
                .as_object()
                .unwrap()
                .iter()
                .filter(|(_, v)| !v.is_null())
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<serde_json::Map<_, _>>()
                .into();
            if let Some(net) = s.src.net() {
                let mut remaining = sessions;
                remaining.reverse();
                let refreshed = remaining.is_empty();
                let pending = if let Some(session) = remaining.pop() {
                    net.dispatch(serde_json::json!({"type":"provider-session.detach","threadId":id,"providerSessionId":session,"reason":"client-requested"}))
                } else {
                    net.request("server.refreshProviders", refresh.clone())
                };
                s.ui.shell.palette.restart = Some(Restart {
                    thread: id,
                    remaining,
                    pending,
                    refresh,
                    refreshed,
                });
            }
        }
    }
    s.ui.shell.palette.open = false;
}
fn toggle_key(s: &App, e: &KeyEvent) -> bool {
    if let Some(bindings) = s
        .state()
        .config
        .as_ref()
        .and_then(|v| v["keybindings"].as_array())
    {
        return bindings
            .iter()
            .filter(|v| v["command"] == "commandPalette.toggle")
            .any(|v| {
                let k = &v["shortcut"];
                let yes = |name: &str| k[name].as_bool().unwrap_or(false);
                let key = k["key"].as_str().unwrap_or("");
                let matches = match &e.key {
                    Key::Command(c) => key == c.to_string(),
                    Key::Text(c) => key == c,
                    _ => false,
                };
                matches
                    && e.mods.cmd == (yes("modKey") || yes("metaKey"))
                    && e.mods.shift == yes("shiftKey")
                    && e.mods.alt == yes("altKey")
                    && (!yes("ctrlKey") || e.mods.ctrl)
            });
    }
    e.is_command('k') && !e.mods.shift && !e.mods.alt
}
pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if toggle_key(s, e) {
        let ui = &mut s.ui.shell.palette;
        ui.open = !ui.open;
        ui.search_due = None;
        ui.message_matches.clear();
        ui.query = TextEdit::default();
        ui.page.clear();
        ui.selected = 0;
        if ui.open {
            ui.query.focused = true;
        }
        return true;
    }
    if !s.ui.shell.palette.open {
        return false;
    }
    let rows = items(s);
    let ui = &mut s.ui.shell.palette;
    match e.key {
        Key::Escape => {
            if !ui.page.is_empty() {
                ui.page.clear();
                ui.query = TextEdit::default();
            } else {
                ui.open = false;
            }
        }
        Key::Backspace if ui.query.text.is_empty() && !ui.page.is_empty() => {
            ui.page.clear();
            ui.selected = 0;
        }
        Key::Down => {
            ui.selected = if rows.is_empty() {
                0
            } else {
                (ui.selected + 1) % rows.len()
            }
        }
        Key::Up => {
            ui.selected = if rows.is_empty() {
                0
            } else {
                (ui.selected + rows.len() - 1) % rows.len()
            }
        }
        Key::Enter => {
            if let Some(item) = rows.get(ui.selected) {
                execute(s, item.action.clone());
            }
            return true;
        }
        _ => return false,
    }
    tx.scroll_into_view(format!("palette-item-{}", s.ui.shell.palette.selected));
    true
}
pub fn overlay(s: &App, _cx: &mut Cx<App>, t: &T) -> Vec<El> {
    if !s.ui.shell.palette.open {
        return s
            .ui
            .shell
            .palette
            .notice
            .as_ref()
            .map(|(title, _)| {
                kit::ToastRegion::new()
                    .toast(kit::Toast::new("palette-notice", title).build(&kit::theme(t)))
                    .build(&kit::theme(t))
            })
            .into_iter()
            .collect();
    }
    let ui = &s.ui.shell.palette;
    let rows = items(s);
    let input = text_input(&ui.query, theme::TEXT_SM)
        .key("palette-query")
        .w_full()
        .h(34.)
        .py(7.)
        .font(theme::TEXT_SM)
        .color(t.c.contrast_foreground)
        .placeholder(
            "Search commands, projects, and threads...",
            t.c.contrast_placeholder,
        )
        .on_input_key(on_key)
        .on_edit(|s: &mut App| &mut s.ui.shell.palette.query)
        .on_input_change(search_changed);
    let head = row()
        .w_full()
        .h(48.)
        .px(17.)
        .gap(12.)
        .center_y()
        .child(icon(&icons::SEARCH, 16.).color(t.c.contrast_icon_muted))
        .child(input);
    let mut list = col().w_full().padding(8.);
    let mut group = "";
    for (i, v) in rows.iter().enumerate() {
        if group != v.group {
            group = v.group;
            list = list.child(
                row().w_full().h(28.).px(8.).center_y().child(
                    text(group)
                        .font(theme::XS_MEDIUM)
                        .color(t.c.contrast_muted_foreground),
                ),
            );
        }
        let action = v.action.clone();
        let mut label = col().grow(1.).child(
            text(v.title.clone())
                .font(theme::TEXT_SM)
                .color(t.c.contrast_foreground),
        );
        if let Some(subtitle) = &v.subtitle {
            label = label.child(
                text(subtitle)
                    .font(theme::TEXT_XS)
                    .color(t.c.contrast_muted_foreground),
            );
        }
        let mut entry = row()
            .key(format!("palette-item-{i}"))
            .role("option")
            .label(v.title.clone())
            .w_full()
            .h(if v.subtitle.is_some() { 50. } else { 32. })
            .px(8.)
            .gap(8.)
            .center_y()
            .radius(6.)
            .fill(if ui.selected == i {
                t.c.accent
            } else {
                rune::Color::TRANSPARENT
            })
            .hover(t.c.accent)
            .on_press(move |s: &mut App| execute(s, action.clone()))
            .child(icon(v.glyph, 16.).color(t.c.contrast_icon_muted))
            .child(label);
        if let Some(timestamp) = &v.timestamp {
            entry = entry.child(
                text(timestamp)
                    .font(theme::TEXT_XS)
                    .color(t.c.contrast_muted_foreground),
            );
        }
        if !v.shortcut.is_empty() {
            entry = entry.child(
                text(v.shortcut)
                    .font(theme::XS_MEDIUM)
                    .color(t.c.contrast_secondary_label),
            );
        }
        if matches!(v.action, Action::Page(_)) {
            entry = entry.child(icon(&icons::CHEVRON_RIGHT, 14.).color(t.c.contrast_icon_muted));
        }
        list = list.child(entry);
    }
    if rows.is_empty() {
        list = list.child(kit::empty("No results found.", "", t));
    }
    let list = kit::scroll_area(
        "palette-results",
        list.max_h(330.).scroll(ScrollSpec {
            stick_bottom: false,
        }),
        t,
    );
    let footer = row()
        .w_full()
        .h(41.)
        .pl(17.)
        .pr(16.)
        .gap(12.)
        .center_y()
        .fill(t.c.contrast_foreground.alpha_mul(0.025))
        .border(1., t.c.contrast_foreground.alpha_mul(0.07))
        .child(
            row()
                .gap(4.)
                .center_y()
                .child(footer_key(Some(&icons::ARROW_UP), "", t))
                .child(footer_key(Some(&icons::ARROW_DOWN), "", t))
                .child(footer_label("Navigate", t)),
        )
        .child(
            row()
                .gap(4.)
                .center_y()
                .child(footer_key(None, "Enter", t))
                .child(footer_label("Select", t)),
        )
        .child(
            row()
                .gap(4.)
                .center_y()
                .child(footer_key(None, "Esc", t))
                .child(footer_label("Close", t)),
        );
    let panel = col()
        .key("command-palette")
        .role("dialog")
        .label("Command palette")
        .transition(rune::Transition::scale(0.98).combined(rune::Transition::opacity()))
        .w(576.)
        .max_h(420.)
        .radius(16.)
        .clip_rounded(16.)
        .fill(t.c.background.alpha_mul(0.8))
        .backdrop_blur(16.)
        .border(1., t.c.contrast_foreground.alpha_mul(0.1))
        .child(head)
        .child(list)
        .child(footer)
        .modal(true, |s: &mut App| s.ui.shell.palette.open = false);
    vec![
        el().key("palette-backdrop")
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .fill(t.c.background.alpha_mul(if t.dark { 0.64 } else { 0.6 }))
            .backdrop_blur(4.),
        col()
            .key("palette-layer")
            .absolute(0., 0.)
            .w_full()
            .h_full()
            .pt(90.)
            .items_center()
            .child(panel),
    ]
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn names_match_word_starts_joined_words_and_restricted_typos() {
        for (name, query, expected) in [
            ("GitHub", "githbu", true),
            ("Open settings", "settings", true),
            ("VS Code", "vscode", true),
            ("JSONSchema", "schema", true),
            ("Search", "arch", true),
            ("Model", "del", false),
            ("PR 123", "124", false),
            ("Thread", "xyz", false),
            ("Café déjà", "cafe deja", true),
            ("Julius’s project", "julius's", true),
            ("配置更新", "更新", true),
            ("Open VS Code", "vscode", true),
            ("Fix in thread", "inth", false),
        ] {
            assert_eq!(score(name, query).is_some(), expected, "{name} / {query}");
        }
        assert!(
            score("Settings", "settings").unwrap() > score("Open settings", "settings").unwrap()
        );
    }
    #[test]
    fn context_matches_do_not_outrank_titles_and_commands_reject_mid_word() {
        assert_eq!(token_score("Search", "arch", false, false), 0.);
        assert!(token_score("Search", "arch", false, true) > 0.);
        assert_eq!(token_score("PR 12345", "12346", true, true), 0.);
        assert_eq!(normalize_query(" “Open   settings” "), "open settings");
        let mut s = App::new();
        s.ui.shell.palette.query = TextEdit::new(data::s(&s.state().projects[0], "title"));
        let rows = items(&s);
        assert!(rows.iter().any(|v| v.group == "Threads"));
        assert!(
            rows.iter()
                .filter(|v| v.group == "Threads")
                .all(|v| v.usage >= 1. && v.usage <= 2.5)
        );
    }
    #[test]
    fn picks_expire_and_exact_picks_beat_newer_prefix_picks() {
        let now = 30 * 86_400_000;
        let thread = serde_json::json!({"opens":{"picks":[
            {"query":"settings","at":now-1000},
            {"query":"settings theme","at":now},
            {"query":"expired","at":0}
        ]}});
        assert_eq!(active_pick(&thread, "settings", now), (now - 1000, true));
        assert_eq!(active_pick(&thread, "set", now), (now, false));
        assert_eq!(active_pick(&thread, "se", now), (0, false));
        assert_eq!(active_pick(&thread, "expired", now), (0, false));
        assert_eq!(thread_query("pull request #123"), "123");
        assert_eq!(thread_query("t3/fix-menu"), "fix-menu");
        let mut app = App::new();
        app.ui.shell.palette.page = "themes".into();
        app.ui.shell.palette.query = TextEdit::new("dark");
        let rows = items(&app);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].title, "Dark");
    }
    #[test]
    fn stale_search_results_are_hidden_after_query_changes() {
        let mut s = App::new();
        s.ui.shell.palette.query = TextEdit::new("needle");
        s.ui.shell.palette.message_query = "old".into();
        s.ui.shell.palette.message_matches =
            vec![serde_json::json!({"threadId":"other","snippet":"old needle"})];
        assert!(!items(&s).iter().any(|v| v.group == "Messages"));
        s.ui.shell.palette.message_query = "needle".into();
        assert!(items(&s).iter().any(|v| v.group == "Messages"));
    }
    #[test]
    fn configured_shortcut_replaces_default_and_requires_exact_modifiers() {
        let mut s = App::new();
        s.src = crate::data::Source::Fixture({
            let mut state = crate::data::fixture_state();
            state.config = Some(
                serde_json::json!({"keybindings":[{"command":"commandPalette.toggle","shortcut":{"key":"p","modKey":true,"shiftKey":true}}]}),
            );
            state
        });
        let mut e = KeyEvent {
            key: Key::Command('p'),
            mods: rune::Mods {
                cmd: true,
                shift: true,
                ..Default::default()
            },
        };
        assert!(toggle_key(&s, &e));
        e.mods.shift = false;
        assert!(!toggle_key(&s, &e));
        e.key = Key::Command('k');
        assert!(!toggle_key(&s, &e));
    }
}

fn footer_key(glyph: Option<&'static rune::Icon>, label: &str, t: &T) -> El {
    let key = row()
        .role("note")
        .center()
        .h(20.)
        .min_w(20.)
        .px(4.)
        .radius(4.)
        .fill(t.c.contrast_foreground.alpha_mul(0.08));
    match glyph {
        Some(glyph) => key.child(icon(glyph, 12.).color(t.c.contrast_foreground)),
        None => key.child(text(label).font(theme::XS_MEDIUM).color(t.c.contrast_foreground)),
    }
}

fn footer_label(label: &str, t: &T) -> El {
    text(label)
        .font(theme::SM_MEDIUM)
        .color(t.c.contrast_muted_foreground)
}
