//! The pull requests Filters menu's Author and Labels pickers (PullRequestListFilters): a search
//! box over the loaded rows' authors and labels, widened by what the hosts find for the search.

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rune::{Color, Cursor, El, col, el, icon, row, text, text_input};
use serde_json::{Value, json};

use crate::App;
use crate::data::s as field;
use crate::icons as i;
use crate::kit::{MenuEntry, MenuItem};
use crate::theme::{self, AlphaMul, T, radius};

/// The picker's search reaches the hosts this long after the last keystroke (OPTION_SEARCH_DEBOUNCE_MS).
const DEBOUNCE: Duration = Duration::from_millis(250);
/// How many loaded matches come before the hosts' answers (HOSTED_CHOICE_ROOM).
const HOSTED_ROOM: usize = 15;
/// The most authors the picker lists.
const AUTHOR_ROWS: usize = 30;
/// The menu keeps at most this many labels (`labels.slice(0, 10)`).
const MAX_LABELS: usize = 10;

#[derive(Clone, Debug, PartialEq)]
pub struct AuthorFacet {
    pub actor: Value,
    pub count: usize,
    pub merged: usize,
}

#[derive(Clone, Debug, PartialEq)]
pub struct LabelFacet {
    pub name: String,
    pub color: Option<String>,
    pub count: usize,
}

/// The open picker's search and the hosts' answer to it (useHostedFilterOptions).
#[derive(Default)]
pub struct Picker {
    pub query: rune::TextEdit,
    /// The typed search last seen, when it changed while not yet sent, and the search sent.
    typed: String,
    typed_at: Option<Instant>,
    sent: String,
    /// `pullRequests.filterOptions` last asked for, the request in flight, and the latest answer.
    asked: Option<Value>,
    pending: Option<u64>,
    answer: Option<(Value, Result<Value, Value>)>,
}

/// What the hosts found for the picker's search, while it matches the typing.
pub struct Hosted {
    pub labels: Vec<Value>,
    pub authors: Vec<Value>,
    pub failed: bool,
    pub pending: bool,
}

fn normalize(v: &str) -> Option<String> {
    let v = v.trim().to_lowercase();
    (!v.is_empty()).then_some(v)
}

fn login(actor: &Value) -> &str {
    actor["login"].as_str().unwrap_or("")
}

/// `localeCompare` closely enough for logins and label names: case folded first.
fn by_name(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

/// collectPullRequestListFacets: every author with a row in `state` (merges loaded first), and the
/// labels on those rows, most used first.
pub fn facets<'a>(entries: impl IntoIterator<Item = &'a Value>, state: &str) -> (Vec<AuthorFacet>, Vec<LabelFacet>) {
    let mut unique: Vec<&Value> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for e in entries {
        let k = super::pull_requests::key(e);
        match at.get(&k) {
            Some(&n) => unique[n] = e,
            None => {
                at.insert(k, unique.len());
                unique.push(e);
            }
        }
    }
    let (mut authors, mut author_at): (Vec<AuthorFacet>, HashMap<String, usize>) = (Vec::new(), HashMap::new());
    let (mut labels, mut label_at): (Vec<LabelFacet>, HashMap<String, usize>) = (Vec::new(), HashMap::new());
    for e in unique {
        let in_state = state == "all" || field(e, "state") == state;
        if e["author"].is_object()
            && let Some(k) = normalize(login(&e["author"]))
        {
            let n = *author_at.entry(k).or_insert_with(|| {
                authors.push(AuthorFacet { actor: e["author"].clone(), count: 0, merged: 0 });
                authors.len() - 1
            });
            authors[n].count += usize::from(in_state);
            authors[n].merged += usize::from(field(e, "state") == "merged");
        }
        if !in_state {
            continue;
        }
        for l in e["labels"].as_array().into_iter().flatten() {
            let Some(k) = normalize(field(l, "name")) else { continue };
            let color = l["color"].as_str().map(str::to_string);
            match label_at.get(&k) {
                Some(&n) => {
                    labels[n].count += 1;
                    if labels[n].color.is_none() {
                        labels[n].color = color;
                    }
                }
                None => {
                    label_at.insert(k, labels.len());
                    labels.push(LabelFacet { name: field(l, "name").to_string(), color, count: 1 });
                }
            }
        }
    }
    authors.retain(|a| a.count > 0);
    authors.sort_by(|a, b| b.merged.cmp(&a.merged).then(b.count.cmp(&a.count)).then_with(|| by_name(login(&a.actor), login(&b.actor))));
    labels.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| by_name(&a.name, &b.name)));
    (authors, labels)
}

/// pullRequestAuthorChoices: the loaded authors matching `query` keep their counts and lead, then
/// whoever the hosts found that no loaded row carries.
pub fn author_choices(facets: &[AuthorFacet], hosted: &[Value], query: &str) -> Vec<AuthorFacet> {
    let needle = normalize(query);
    let hit = |f: &&AuthorFacet| {
        needle.as_deref().is_none_or(|n| login(&f.actor).to_lowercase().contains(n) || f.actor["name"].as_str().is_some_and(|name| name.to_lowercase().contains(n)))
    };
    let mut loaded: Vec<AuthorFacet> = facets.iter().filter(hit).cloned().collect();
    let mut seen: Vec<Option<String>> = loaded.iter().map(|f| normalize(login(&f.actor))).collect();
    let mut added = Vec::new();
    for actor in hosted {
        let k = normalize(login(actor));
        if seen.contains(&k) {
            continue;
        }
        seen.push(k.clone());
        added.push(facets.iter().find(|f| normalize(login(&f.actor)) == k).cloned().unwrap_or(AuthorFacet { actor: actor.clone(), count: 0, merged: 0 }));
    }
    let rest = loaded.split_off(loaded.len().min(HOSTED_ROOM));
    loaded.extend(added);
    loaded.extend(rest);
    loaded
}

/// selectedPullRequestAuthor: the filter's author as a choice, even when no loaded row carries it.
fn selected_author(facets: &[AuthorFacet], hosted: &[Value], value: Option<&str>) -> Option<AuthorFacet> {
    let value = value?;
    let k = normalize(value);
    Some(facets.iter().find(|f| normalize(login(&f.actor)) == k).cloned().unwrap_or_else(|| AuthorFacet {
        actor: hosted.iter().find(|a| normalize(login(a)) == k).cloned().unwrap_or_else(|| json!({"login": value, "name": null, "avatarUrl": null})),
        count: 0,
        merged: 0,
    }))
}

/// The author picker's rows: the chosen author first, then the other matches, thirty at most.
pub fn visible_authors(facets: &[AuthorFacet], hosted: &[Value], query: &str, value: Option<&str>) -> Vec<AuthorFacet> {
    let chosen = value.map(str::to_lowercase).unwrap_or_default();
    selected_author(facets, hosted, value)
        .into_iter()
        .chain(author_choices(facets, hosted, query).into_iter().filter(|o| login(&o.actor).to_lowercase() != chosen))
        .take(AUTHOR_ROWS)
        .collect()
}

/// pullRequestLabelChoices: the loaded labels matching `query`, then the hosts' other finds.
pub fn label_choices(facets: &[LabelFacet], hosted: &[Value], query: &str) -> Vec<LabelFacet> {
    let needle = normalize(query);
    let mut out: Vec<LabelFacet> = facets.iter().filter(|f| needle.as_deref().is_none_or(|n| f.name.to_lowercase().contains(n))).cloned().collect();
    let mut seen: Vec<Option<String>> = out.iter().map(|f| normalize(&f.name)).collect();
    for label in hosted {
        let k = normalize(field(label, "name"));
        if seen.contains(&k) {
            continue;
        }
        seen.push(k.clone());
        out.push(facets.iter().find(|f| normalize(&f.name) == k).cloned().unwrap_or(LabelFacet {
            name: field(label, "name").to_string(),
            color: label["color"].as_str().map(str::to_string),
            count: 0,
        }));
    }
    out
}

/// The label picker's rows: chosen labels no choice offers, then the choices.
pub fn visible_labels(value: &[String], facets: &[LabelFacet], hosted: &[Value], query: &str) -> Vec<LabelFacet> {
    let choices = label_choices(facets, hosted, query);
    let missing: Vec<LabelFacet> = value
        .iter()
        .filter(|v| !choices.iter().any(|c| c.name.to_lowercase() == v.to_lowercase()))
        .map(|name| LabelFacet { name: name.clone(), color: None, count: 0 })
        .collect();
    missing.into_iter().chain(choices).collect()
}

/// The chosen labels after (un)ticking `name`, capped like the menu.
pub fn toggled(value: &[String], name: &str, on: bool) -> Vec<String> {
    let mut next: Vec<String> = value.iter().filter(|v| on || v.to_lowercase() != name.to_lowercase()).cloned().collect();
    if on {
        next.push(name.to_string());
    }
    next.truncate(MAX_LABELS);
    next
}

/// pullRequestLabelColor: a six-digit hex, with or without its `#`.
fn label_hex(color: Option<&str>) -> Option<String> {
    let color = color?.trim();
    let hex = color.strip_prefix('#').unwrap_or(color);
    (hex.len() == 6 && hex.bytes().all(|b| b.is_ascii_hexdigit())).then(|| format!("#{hex}"))
}

/// One Project filter choice: the project, the title the menu shows, and why its listing failed.
#[derive(Clone, Debug, PartialEq)]
pub struct ProjectChoice {
    pub id: String,
    pub title: String,
    pub unavailable: Option<String>,
}

/// pullRequestFilterProjects then PullRequestFiltersMenu's order: one project per repository (the
/// chosen one where two share it), shared titles told apart by server, then folder, then id,
/// sorted by title, and the projects whose listing failed last.
pub fn projects(projects: &[Value], selected: Option<&str>, server: &str, errors: &[Value]) -> Vec<ProjectChoice> {
    let mut kept: Vec<&Value> = Vec::new();
    let mut at: HashMap<String, usize> = HashMap::new();
    for p in projects.iter().filter(|p| p["isScratch"] != true) {
        let id = field(p, "id");
        let k = match p["repositoryIdentity"]["canonicalKey"].as_str().filter(|r| !r.is_empty()) {
            Some(repository) => format!("repository\0{}", repository.to_lowercase()),
            None => format!("project\0{id}"),
        };
        match at.get(&k) {
            Some(&n) if selected == Some(id) => kept[n] = p,
            Some(_) => {}
            None => {
                at.insert(k, kept.len());
                kept.push(p);
            }
        }
    }
    let mut titled: Vec<(String, &Value)> = kept.into_iter().map(|p| (field(p, "title").to_string(), p)).collect();
    for stage in 0..3 {
        let mut counts: HashMap<String, usize> = HashMap::new();
        for (title, _) in &titled {
            *counts.entry(title.clone()).or_default() += 1;
        }
        for (title, p) in titled.iter_mut() {
            if counts.get(title.as_str()).copied().unwrap_or(0) > 1 {
                let suffix = match stage {
                    0 => server,
                    1 => field(p, "workspaceRoot"),
                    _ => field(p, "id"),
                };
                *title = format!("{title} · {suffix}");
            }
        }
    }
    titled.sort_by(|a, b| by_name(&a.0, &b.0));
    let mut out: Vec<ProjectChoice> = titled
        .into_iter()
        .map(|(title, p)| {
            let id = field(p, "id").to_string();
            let unavailable = errors.iter().find(|e| field(e, "projectId") == id).map(|e| field(e, "message").to_string());
            ProjectChoice { id, title, unavailable }
        })
        .collect();
    out.sort_by_key(|c| c.unavailable.is_some());
    out
}

/// The Project submenu: "All projects", then each project under its favicon; one whose listing
/// failed is greyed out with the reason on hover. Picking keeps the menu open.
pub fn project_entries(s: &App, t: &T, choices: &[ProjectChoice]) -> Vec<MenuEntry> {
    let chosen = s.ui.pages.pull_requests.filters.project.as_deref();
    let mut out = vec![
        MenuEntry::Label("Project".into()),
        MenuEntry::item(MenuItem::new("All projects").icon(&i::LAYERS).radio(chosen.is_none()).on_select(|s: &mut App| s.ui.pages.pull_requests.filters.project = None)),
    ];
    for c in choices {
        let checked = chosen == Some(c.id.as_str());
        let mut line = choice(t, format!("pr-project-{}", c.id), "menuitemradio", c.title.clone(), checked)
            .fill(if checked { t.c.foreground.alpha_mul(0.08) } else { Color::TRANSPARENT })
            .child(crate::shell::sidebar::favicon::mark(s, t, &c.id, &c.title, false, 14.0))
            .child(text(c.title.clone()).font(theme::TEXT_SM).color(t.c.foreground).truncate().grow(1.0).shrink().min_w(0.0));
        line = match &c.unavailable {
            Some(why) => line.child(text("· Unavailable").font(theme::TEXT_SM).color(t.c.foreground).shrink()).child(tick(t, checked)).disabled(true).focusable(false).opacity(0.64).help(why.clone()),
            None => {
                let id = c.id.clone();
                line.child(tick(t, checked)).on_press(move |s: &mut App| s.ui.pages.pull_requests.filters.project = Some(id.clone()))
            }
        };
        out.push(MenuEntry::Row(line));
    }
    out
}

/// The search sent for `text`: trimmed and capped at the input's 100 characters.
fn typed(text: &str) -> String {
    text.trim().chars().take(100).collect::<String>().trim_end().to_string()
}

/// `pullRequests.filterOptions` over the listing's own scope.
fn input(kind: &str, project: Option<&str>, host: Option<&str>, sent: &str) -> Value {
    let mut v = json!({"kind": kind});
    if let Some(p) = project {
        v["projectId"] = json!(p);
    }
    if let Some(h) = host {
        v["host"] = json!(h);
    }
    if !sent.is_empty() {
        v["query"] = json!(sent);
    }
    v
}

/// The hosts' options for the open picker: none yet, and busy, until the search catches up.
pub fn hosted(p: &Picker, kind: &str, project: Option<&str>, host: Option<&str>) -> Hosted {
    let none = |pending| Hosted { labels: Vec::new(), authors: Vec::new(), failed: false, pending };
    if typed(&p.query.text) != p.sent {
        return none(true);
    }
    let want = input(kind, project, host, &p.sent);
    let pending = p.pending.is_some() && p.asked.as_ref() == Some(&want);
    let list = |v: &Value, key: &str| v[key].as_array().cloned().unwrap_or_default();
    match p.answer.as_ref().filter(|a| a.0 == want).map(|a| &a.1) {
        Some(Ok(v)) => Hosted { labels: list(v, "labels"), authors: list(v, "authors"), failed: false, pending },
        Some(Err(_)) => Hosted { failed: true, ..none(pending) },
        None => none(pending),
    }
}

/// The authors the hosts found, whose avatars the picker shows.
pub fn hosted_actors(p: &Picker) -> impl Iterator<Item = &Value> {
    p.answer.iter().filter_map(|a| a.1.as_ref().ok()).flat_map(|v| v["authors"].as_array().into_iter().flatten())
}

/// Opening a picker starts a fresh search with the caret in it (`autoFocus`); answers stay cached.
pub fn open(p: &mut Picker) {
    p.query = rune::TextEdit::default();
    p.query.focused = true;
    p.typed.clear();
    p.sent.clear();
    p.typed_at = None;
}

/// Records `answer` as the hosts' reply to the open picker's empty search (scenes).
pub fn hold(p: &mut Picker, kind: &str, project: Option<&str>, host: Option<&str>, answer: Value) {
    let asked = input(kind, project, host, "");
    p.asked = Some(asked.clone());
    p.answer = Some((asked, Ok(answer)));
}

/// Sends the open picker's search once typing pauses, and folds the hosts' answer.
pub fn poll(s: &mut App) -> bool {
    let u = &s.ui.pages.pull_requests;
    let kind = match (u.menu, u.submenu) {
        (Some("filters"), Some("author")) => Some("authors"),
        (Some("filters"), Some("labels")) => Some("labels"),
        _ => None,
    };
    let (project, host) = (u.filters.project.clone(), u.filters.host.clone());
    let waker = s.ui.panel.waker.clone();
    let (net, p) = (s.src.net(), &mut s.ui.pages.pull_requests.picker);
    let mut changed = false;
    let now = typed(&p.query.text);
    if now != p.typed {
        p.typed = now;
        p.typed_at = Some(Instant::now());
        if let Some(waker) = waker {
            std::thread::spawn(move || {
                std::thread::sleep(DEBOUNCE);
                waker();
            });
        }
    }
    if p.typed_at.is_some_and(|at| at.elapsed() >= DEBOUNCE) {
        p.typed_at = None;
        p.sent = p.typed.clone();
        changed = true;
    }
    let Some(net) = net else { return changed };
    if let Some(id) = p.pending
        && let Some(reply) = net.take_reply(id)
    {
        p.pending = None;
        if let Some(asked) = p.asked.clone() {
            p.answer = Some((asked, reply));
        }
        changed = true;
    }
    if let Some(kind) = kind {
        let want = input(kind, project.as_deref(), host.as_deref(), &p.sent);
        if p.asked.as_ref() != Some(&want) {
            p.pending = Some(net.request("pullRequests.filterOptions", want.clone()));
            p.asked = Some(want);
            changed = true;
        }
    }
    changed
}

/// MenuSearchInput: a search icon, or a spinner while the hosts search, over an underline.
fn search_box(t: &T, p: &Picker, key: &'static str, placeholder: &'static str, busy: bool) -> MenuEntry {
    let line = if p.query.focused { t.c.ring } else { t.c.border.alpha_mul(0.7) };
    let field = row()
        .w_full()
        .h(26.0)
        .center_y()
        .child(icon(if busy { &i::LOADER_CIRCLE } else { &i::SEARCH }, 16.0).color(t.c.muted_foreground.alpha_mul(0.55)).absolute(0.0, 5.0))
        .child(
            text_input(&p.query, theme::TEXT_SM.lh(26.0))
                .key(key)
                .single_line()
                .w_full()
                .pl(20.0)
                .color(t.c.foreground)
                .placeholder(placeholder, t.c.placeholder)
                .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
                .on_edit(|s: &mut App| &mut s.ui.pages.pull_requests.picker.query),
        );
    MenuEntry::Row(col().w_full().px(8.0).pt(6.0).pb(4.0).child(col().w_full().pb(6.0).child(field)).child(el().w_full().h(1.0).fill(line)))
}

/// A picker row in the menu's item geometry, highlighted like `data-highlighted:bg-accent`.
fn choice(t: &T, key: String, role: &'static str, label: String, checked: bool) -> El {
    row()
        .key(key)
        .role(role)
        .label(label)
        .checked(checked)
        .w_full()
        .min_h(28.0)
        .px(8.0)
        .py(4.0)
        .gap(8.0)
        .center_y()
        .radius(4.0)
        .cursor(Cursor::Pointer)
        .hover(t.c.accent)
        .focusable(true)
        .focus_highlight(t.c.accent)
}

fn tick(t: &T, on: bool) -> El {
    el().size(16.0, 16.0).center().child(icon(&i::CHECK, 14.0).color(t.c.foreground).opacity(if on { 1.0 } else { 0.0 }))
}

fn count_text(t: &T, value: String) -> El {
    text(value).font(theme::TEXT_XS.features("'tnum' 1")).color(t.c.muted_foreground).shrink()
}

/// PullRequestAuthorChoices: the search, "Anyone", then the authors; picking one keeps the menu open.
pub fn author_entries(s: &App, t: &T, facets: &[AuthorFacet]) -> Vec<MenuEntry> {
    let u = &s.ui.pages.pull_requests;
    let value = u.filters.author.as_deref();
    let hosted = hosted(&u.picker, "authors", u.filters.project.as_deref(), u.filters.host.as_deref());
    let visible = visible_authors(facets, &hosted.authors, &u.picker.query.text, value);
    let mut out = vec![
        search_box(t, &u.picker, "pr-author-search", "Search authors", hosted.pending),
        MenuEntry::item(MenuItem::new("Anyone").icon(&i::LAYERS).radio(value.is_none()).on_select(|s: &mut App| s.ui.pages.pull_requests.filters.author = None)),
    ];
    let chosen = value.map(str::to_lowercase).unwrap_or_default();
    for option in visible.iter() {
        let name = login(&option.actor).to_string();
        let checked = name.to_lowercase() == chosen;
        let mut line = choice(t, format!("pr-author-{}", name.to_lowercase()), "menuitemradio", name.clone(), checked)
            .fill(if checked { t.c.foreground.alpha_mul(0.08) } else { Color::TRANSPARENT })
            .child(super::avatars::avatar(&s.ui.pages.avatars, &option.actor, 16.0, t))
            .child(text(name.clone()).font(theme::TEXT_SM).color(t.c.foreground).truncate().grow(1.0).shrink().min_w(0.0));
        if option.merged > 0 {
            line = line.child(count_text(t, format!("{} merges loaded", option.merged)));
        }
        line = line.child(tick(t, checked)).on_press(move |s: &mut App| {
            let f = &mut s.ui.pages.pull_requests.filters;
            if f.author.as_deref().map(str::to_lowercase) != Some(name.to_lowercase()) {
                f.author = Some(name.clone());
            }
        });
        out.push(MenuEntry::Row(line));
    }
    if visible.is_empty() && !hosted.pending {
        out.push(MenuEntry::item(MenuItem::new(if hosted.failed { "Couldn't search authors on the host" } else { "No authors found" }).disabled(true)));
    }
    out
}

/// PullRequestLabelChoices: the search, then every label as a checkbox; the menu stays open.
pub fn label_entries(s: &App, t: &T, facets: &[LabelFacet]) -> Vec<MenuEntry> {
    let u = &s.ui.pages.pull_requests;
    let value = &u.filters.labels;
    let hosted = hosted(&u.picker, "labels", u.filters.project.as_deref(), u.filters.host.as_deref());
    let visible = visible_labels(value, facets, &hosted.labels, &u.picker.query.text);
    let mut out = vec![search_box(t, &u.picker, "pr-label-search", "Search labels", hosted.pending)];
    if visible.is_empty() && !hosted.pending {
        out.push(MenuEntry::item(MenuItem::new(if hosted.failed { "Couldn't search labels on the host" } else { "No labels found" }).disabled(true)));
    }
    for option in visible {
        let checked = value.iter().any(|v| v.to_lowercase() == option.name.to_lowercase());
        let dot = label_hex(option.color.as_deref()).and_then(|h| rune::color::hex(&h)).unwrap_or(t.c.muted_foreground);
        let mut line = choice(t, format!("pr-label-{}", option.name.to_lowercase()), "menuitemcheckbox", option.name.clone(), checked)
            .pr(16.0)
            .child(tick(t, checked))
            .child(el().size(10.0, 10.0).shrink().radius(radius::FULL).fill(dot))
            .child(text(option.name.clone()).font(theme::TEXT_SM).color(t.c.foreground).truncate().grow(1.0).shrink().min_w(0.0));
        if option.count > 0 {
            line = line.child(count_text(t, option.count.to_string()));
        }
        let name = option.name;
        out.push(MenuEntry::Row(line.on_press(move |s: &mut App| {
            let f = &mut s.ui.pages.pull_requests.filters;
            f.labels = toggled(&f.labels, &name, !checked);
        })));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(number: u64, state: &str, login: &str, labels: &[&str]) -> Value {
        let labels: Vec<Value> = labels.iter().map(|l| json!({"name": l, "color": null})).collect();
        json!({"host": "github.com", "repository": "acme/app", "number": number, "state": state, "author": {"login": login, "name": null, "avatarUrl": null}, "labels": labels})
    }

    #[test]
    fn facets_count_rows_in_the_state_and_rank_authors_by_merges_loaded() {
        let rows = [
            entry(1, "open", "Octo", &["Bug", "ui"]),
            entry(2, "open", "octo", &["bug"]),
            entry(3, "merged", "mara", &["docs"]),
            entry(4, "open", "mara", &[]),
            entry(5, "closed", "lena", &["wip"]),
            entry(1, "open", "Octo", &["Bug", "ui"]),
        ];
        let (authors, labels) = facets(rows.iter(), "open");
        let summary: Vec<(&str, usize, usize)> = authors.iter().map(|a| (login(&a.actor), a.count, a.merged)).collect();
        assert_eq!(summary, [("mara", 1, 1), ("Octo", 2, 0)]);
        let names: Vec<(&str, usize)> = labels.iter().map(|l| (l.name.as_str(), l.count)).collect();
        assert_eq!(names, [("Bug", 2), ("ui", 1)]);
    }

    #[test]
    fn author_choices_lead_with_loaded_matches_then_the_hosts_finds() {
        let facet = |login: &str| AuthorFacet { actor: json!({"login": login, "name": null}), count: 1, merged: 0 };
        let loaded: Vec<AuthorFacet> = (0..17).map(|n| facet(&format!("dev{n}"))).collect();
        let hosted = [json!({"login": "DEV3"}), json!({"login": "devon"})];
        let choices = author_choices(&loaded, &hosted, "dev");
        let order: Vec<&str> = choices.iter().map(|c| login(&c.actor)).collect();
        assert_eq!(order[14..17], ["dev14", "devon", "dev15"]);
        assert_eq!(choices.len(), 18);
        let visible = visible_authors(&loaded, &hosted, "", Some("Someone"));
        assert_eq!(login(&visible[0].actor), "Someone");
        assert_eq!(visible.len(), AUTHOR_ROWS.min(1 + 18));
        assert_eq!(visible_authors(&loaded, &[], "", Some("DEV2")).iter().filter(|a| login(&a.actor) == "dev2").count(), 1);
    }

    #[test]
    fn label_choices_keep_chosen_labels_and_add_the_hosts_finds() {
        let loaded = vec![LabelFacet { name: "bug".into(), color: Some("d73a4a".into()), count: 3 }];
        let hosted = [json!({"name": "BUG", "color": null}), json!({"name": "docs", "color": "#0075ca"})];
        let rows = visible_labels(&["area:ui".into()], &loaded, &hosted, "");
        let names: Vec<&str> = rows.iter().map(|l| l.name.as_str()).collect();
        assert_eq!(names, ["area:ui", "bug", "docs"]);
        let searched: Vec<(String, usize)> = label_choices(&loaded, &hosted, "do").into_iter().map(|l| (l.name, l.count)).collect();
        assert_eq!(searched, [("bug".to_string(), 3), ("docs".to_string(), 0)]);
        assert_eq!(toggled(&["Bug".into(), "ui".into()], "bug", false), ["ui"]);
        let full: Vec<String> = (0..10).map(|n| n.to_string()).collect();
        assert_eq!(toggled(&full, "extra", true).len(), 10);
        assert_eq!(label_hex(Some("#0075CA")).as_deref(), Some("#0075CA"));
        assert_eq!(label_hex(Some("blue")), None);
    }

    #[test]
    fn project_choices_keep_one_per_repository_tell_titles_apart_and_put_failures_last() {
        let p = |id: &str, title: &str, key: Option<&str>, root: &str| json!({"id": id, "title": title, "workspaceRoot": root, "repositoryIdentity": key.map(|k| json!({"canonicalKey": k}))});
        let list = [
            p("a", "web", Some("github.com/acme/web"), "/w/web"),
            p("b", "web copy", Some("GitHub.com/acme/web"), "/w/web2"),
            p("c", "api", None, "/w/api"),
            p("d", "api", None, "/x/api"),
            p("e", "Docs", None, "/w/docs"),
            json!({"id": "s", "title": "Scratch", "isScratch": true}),
        ];
        let errors = [json!({"projectId": "c", "projectTitle": "api", "message": "gh is not authenticated."})];
        let choices = projects(&list, None, "This machine", &errors);
        let shown: Vec<(&str, &str)> = choices.iter().map(|c| (c.id.as_str(), c.title.as_str())).collect();
        assert_eq!(shown, [("d", "api · This machine · /x/api"), ("e", "Docs"), ("a", "web"), ("c", "api · This machine · /w/api")]);
        assert_eq!(choices[3].unavailable.as_deref(), Some("gh is not authenticated."));
        let chosen: Vec<String> = projects(&list, Some("b"), "This machine", &[]).into_iter().map(|c| c.id).collect();
        assert_eq!(chosen, ["c", "d", "e", "b"]);
    }

    #[test]
    fn hosted_options_wait_for_the_search_to_settle_and_match_the_scope() {
        let mut p = Picker::default();
        hold(&mut p, "labels", None, Some("github.com"), json!({"labels": [{"name": "docs"}], "authors": []}));
        assert_eq!(hosted(&p, "labels", None, Some("github.com")).labels.len(), 1);
        assert!(hosted(&p, "labels", None, None).labels.is_empty());
        p.query.replace_text("do");
        let typing = hosted(&p, "labels", None, Some("github.com"));
        assert!(typing.pending && typing.labels.is_empty());
        assert_eq!(input("authors", Some("p1"), None, "octo"), json!({"kind": "authors", "projectId": "p1", "query": "octo"}));
    }
}
