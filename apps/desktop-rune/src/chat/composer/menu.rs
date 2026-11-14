//! The composer's trigger drawer (ComposerCommandMenu): `@` files and folders from
//! `projects.searchEntries`, `/` commands, `$` skills, `#` the project's pull requests. It rises out of the top of the composer,
//! tucked 17 points under it.

use rune::{Cursor, Cx, El, Tx, col, el, icon, row, text};
use serde_json::{Value, json};

use super::{ctx, draft, draft_mut, provider, selection, set_interaction_mode, supports_plan};
use crate::data;
use crate::icons;
use crate::theme::{self, T, radius};
use crate::App;

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Kind {
    Path,
    Slash,
    Skill,
    PullRequest,
}

/// The trigger token under the caret.
pub struct Open {
    pub kind: Kind,
    pub token: String,
    pub start: usize,
    pub end: usize,
}

impl Open {
    fn query(&self) -> &str {
        &self.token[1..]
    }
}

#[derive(Default)]
pub struct Mention {
    query: Option<String>,
    request: Option<u64>,
    pub loading: bool,
    entries: Vec<(String, bool)>,
    prs: Prs,
}

/// The `#` list: one `pullRequests.list` read per project and text query (numbers filter
/// the unqueried list, as useComposerMenuItems does).
#[derive(Default)]
struct Prs {
    key: Option<(String, String)>,
    request: Option<u64>,
    loading: bool,
    failed: bool,
    entries: Vec<Value>,
}

const PR_RESULTS: usize = 12;

pub struct Item {
    pub id: String,
    pub label: String,
    pub description: String,
    pub folder: Option<bool>,
    pub pr: Option<Value>,
}

const BUILTIN_SLASH: [(&str, &str); 1] = [("model", "Switch response model for this thread")];
/// Shown after `/model` when the provider has plan mode.
const PLAN_SLASH: [(&str, &str); 2] =
    [("plan", "Switch this thread into plan mode"), ("default", "Switch this thread back to normal build mode")];

pub fn open_menu(s: &App) -> Option<Open> {
    let d = draft(s)?;
    let text = &d.edit.text;
    let caret = d.edit.focus.min(text.len());
    if !text.is_char_boundary(caret) {
        return None;
    }
    let start = text[..caret].rfind(char::is_whitespace).map_or(0, |i| i + text[i..].chars().next().map_or(1, char::len_utf8));
    let token = &text[start..caret];
    let kind = match token.chars().next()? {
        '@' => Kind::Path,
        // A second slash makes the word a path (isPathWord).
        '/' if !text[start + 1..].split(char::is_whitespace).next().unwrap_or("").contains('/') => Kind::Slash,
        '$' => Kind::Skill,
        '#' if pr_query(&token[1..]) => Kind::PullRequest,
        _ => return None,
    };
    if s.ui.chat.composer.menu_dismissed.as_deref() == Some(token) {
        return None;
    }
    Some(Open { kind, token: token.to_string(), start, end: caret })
}

fn provider_list<'a>(s: &'a App, key: &str) -> Vec<&'a Value> {
    selection(s).and_then(|(i, _)| provider(s, &i)).and_then(|p| p[key].as_array()).map(|v| v.iter().collect()).unwrap_or_default()
}

pub fn items(s: &App, m: &Open) -> Vec<Item> {
    let q = m.query().to_lowercase();
    let item = |id: String, label: String, description: String| Item { id, label, description, folder: None, pr: None };
    match m.kind {
        Kind::Slash => {
            let plan: &[(&str, &str)] = if supports_plan(s) { &PLAN_SLASH[..] } else { &[] };
            // Provider commands work only at the message start.
            let at_start = m.start == 0;
            let builtins = BUILTIN_SLASH.iter().chain(plan).map(|(n, d)| item(format!("/{n}"), format!("/{n}"), d.to_string()));
            let skills = slash_menu_skills(s);
            let skill_names: Vec<String> = skills.iter().map(|k| data::s(k, "name").trim().to_lowercase()).collect();
            let compact = compact_available(s, m);
            let provided = provider_list(s, "slashCommands")
                .into_iter()
                .filter(|_| at_start)
                .filter(|c| !skill_names.contains(&data::s(c, "name").trim().to_lowercase()))
                .filter(|c| data::s(c, "name") != "compact" || compact)
                .map(|c| item(format!("/{}", data::s(c, "name")), format!("/{}", data::s(c, "name")), command_description(c)));
            // A skill runs anywhere in the prompt, as `$name`.
            let skill_items = skills.iter().map(|k| item(format!("${}", data::s(k, "name")), format!("/skill:{}", data::s(k, "name")), skill_description(k, "")));
            let builtin = BUILTIN_SLASH.len() + plan.len();
            let all: Vec<Item> = builtins.chain(provided).chain(skill_items).collect();
            ranked_slash(all, builtin, q.trim_start_matches('/'))
        }
        Kind::Skill => provider_list(s, "skills")
            .into_iter()
            .map(|c| item(format!("${}", data::s(c, "name")), data::s(c, "name").to_string(), skill_description(c, "Run provider skill")))
            .filter(|i| i.label.to_lowercase().contains(&q))
            .collect(),
        Kind::Path => {
            let mention = &s.ui.chat.composer.mention;
            if mention.query.as_deref() != Some(m.query()) {
                return Vec::new();
            }
            mention
                .entries
                .iter()
                .map(|(path, folder)| {
                    let (dir, name) = path.rsplit_once('/').unwrap_or(("", path));
                    Item { id: path.clone(), label: name.to_string(), description: dir.to_string(), folder: Some(*folder), pr: None }
                })
                .collect()
        }
        Kind::PullRequest => pr_matches(s, m.query())
            .into_iter()
            .map(|e| Item { id: format!("#{}", e["number"]), label: format!("#{}", e["number"]), description: data::s(e, "title").to_string(), folder: None, pr: Some(e.clone()) })
            .collect(),
    }
}

/// A provider command's description, its input hint, or "Run provider command".
fn command_description(c: &Value) -> String {
    [data::s(c, "description"), data::s(&c["input"], "hint")].into_iter().find(|d| !d.is_empty()).unwrap_or("Run provider command").to_owned()
}

/// A skill's short description, its description, its scope, else `fallback`.
fn skill_description(k: &Value, fallback: &str) -> String {
    let scope = data::s(k, "scope");
    let scoped = if scope.is_empty() { String::new() } else { format!("{scope} skill") };
    [data::s(k, "shortDescription").to_owned(), data::s(k, "description").to_owned(), scoped]
        .into_iter()
        .find(|d| !d.is_empty())
        .unwrap_or_else(|| fallback.to_owned())
}

/// getProviderSkillsForSlashMenu: invocable skills, one per name, when Settings lists them in `/`.
fn slash_menu_skills(s: &App) -> Vec<&Value> {
    if crate::settings::value(s, "showSkillsInSlashMenu") == false {
        return Vec::new();
    }
    let mut seen: Vec<String> = Vec::new();
    let mut out = Vec::new();
    for k in provider_list(s, "skills") {
        let name = data::s(k, "name").trim().to_lowercase();
        if k["enabled"] == true && k["userInvocable"] != false && !seen.contains(&name) {
            seen.push(name);
            out.push(k);
        }
    }
    out
}

/// `/compact` is offered only alone in the prompt, with nothing attached, while compacting can
/// run (compactSlashCommandAvailable).
fn compact_available(s: &App, m: &Open) -> bool {
    let Some(d) = draft(s) else { return false };
    let text = &d.edit.text;
    m.end <= text.len()
        && text[..m.start].trim().is_empty()
        && text[m.end..].trim().is_empty()
        && d.attachments.is_empty()
        && d.edit.pills().next().is_none()
        && matches!(super::context::compact_state(s), Some(None))
}

/// searchSlashCommandItems: by the best of the name's and the description's match, built-ins
/// before provider commands on a tie, then by name.
fn ranked_slash(items: Vec<Item>, builtin: usize, q: &str) -> Vec<Item> {
    let q = q.trim();
    if q.is_empty() {
        return items;
    }
    let mut ranked: Vec<(usize, usize, Item)> = items
        .into_iter()
        .enumerate()
        .filter_map(|(i, it)| {
            let name = score(&it.label[1..].to_lowercase(), q, [0, 2, 4, 6], Some(100), &["-", "_", "/"]);
            let desc = score(&it.description.to_lowercase(), q, [20, 22, 24, 26], None, &[" ", "-", "_", "/"]);
            let best = name.into_iter().chain(desc).min()?;
            Some((best, i, it))
        })
        .collect();
    ranked.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| (a.1 >= builtin).cmp(&(b.1 >= builtin))).then_with(|| a.2.label.cmp(&b.2.label)));
    ranked.into_iter().map(|(_, _, it)| it).collect()
}

/// scoreQueryMatch over lowercased `value`: exact, prefix, word boundary, substring, then a
/// subsequence, each later tier scoring higher (worse) and longer values a little worse.
fn score(value: &str, q: &str, [exact, prefix, boundary, includes]: [usize; 4], fuzzy: Option<usize>, markers: &[&str]) -> Option<usize> {
    if value.is_empty() || q.is_empty() {
        return None;
    }
    let (v, qc): (Vec<char>, Vec<char>) = (value.chars().collect(), q.chars().collect());
    let penalty = v.len().saturating_sub(qc.len()).min(64);
    let char_index = |byte: usize| value[..byte].chars().count();
    if value == q {
        return Some(exact);
    }
    if value.starts_with(q) {
        return Some(prefix + penalty);
    }
    if let Some(at) = markers.iter().filter_map(|m| value.find(&format!("{m}{q}")).map(|i| char_index(i) + m.chars().count())).min() {
        return Some(boundary + at * 2 + penalty);
    }
    if let Some(i) = value.find(q) {
        return Some(includes + char_index(i) * 2 + penalty);
    }
    let base = fuzzy?;
    let (mut qi, mut first, mut prev, mut gaps) = (0, None, None, 0);
    for (vi, c) in v.iter().enumerate() {
        if *c != qc[qi] {
            continue;
        }
        first.get_or_insert(vi);
        if let Some(p) = prev {
            gaps += vi - p - 1;
        }
        prev = Some(vi);
        qi += 1;
        if qi == qc.len() {
            let first = first.unwrap_or(0);
            let span = vi - first + 1 - qc.len();
            return Some(base + first * 2 + gaps * 3 + span + penalty);
        }
    }
    None
}

/// `#` then nothing, or a letter or digit and then letters, digits, `_` or `-`.
fn pr_query(q: &str) -> bool {
    let mut chars = q.chars();
    match chars.next() {
        None => true,
        Some(c) => c.is_alphanumeric() && chars.all(|c| c.is_alphanumeric() || c == '_' || c == '-'),
    }
}

fn numeric(q: &str) -> bool {
    q.chars().all(|c| c.is_ascii_digit())
}

/// filterComposerPullRequestMatches for a number (exact first, then the newest), the server's
/// order for text.
fn pr_matches<'a>(s: &'a App, q: &str) -> Vec<&'a Value> {
    let prs = &s.ui.chat.composer.mention.prs;
    let project = ctx(s).project.map(|p| data::s(p, "id")).unwrap_or_default();
    match &prs.key {
        Some((p, k)) if p == project && *k == pr_key(q) => {}
        _ => return Vec::new(),
    }
    let mut list: Vec<&Value> = prs.entries.iter().filter(|e| !numeric(q) || e["number"].to_string().contains(q)).collect();
    if numeric(q) {
        list.retain(|e| data::s(e, "projectId").is_empty() || data::s(e, "projectId") == project);
        list.sort_by(|a, b| {
            let exact = |e: &Value| e["number"].to_string() == q;
            exact(b).cmp(&exact(a)).then_with(|| data::s(b, "updatedAt").cmp(data::s(a, "updatedAt")))
        });
    }
    list.truncate(PR_RESULTS);
    list
}

fn pr_key(q: &str) -> String {
    if numeric(q) { String::new() } else { q.to_owned() }
}

/// The working directory file searches run in: the thread's worktree, else the project root.
pub(super) fn search_root(s: &App) -> Option<String> {
    let c = ctx(s);
    let worktree = c.thread.map(|t| data::s(t, "worktreePath")).filter(|p| !p.is_empty());
    worktree.or_else(|| c.project.map(|p| data::s(p, "workspaceRoot"))).filter(|p| !p.is_empty()).map(str::to_string)
}

/// Starts a file search when the `@` query changed, or a pull request read for `#`.
pub fn refresh(s: &mut App) {
    let Some(m) = open_menu(s) else { return };
    if m.kind == Kind::PullRequest {
        let Some(project) = ctx(s).project.map(|p| data::s(p, "id").to_string()) else { return };
        let key = (project.clone(), pr_key(m.query()));
        let prs = &mut s.ui.chat.composer.mention.prs;
        if prs.key.as_ref() == Some(&key) {
            return;
        }
        *prs = Prs { key: Some(key.clone()), ..Prs::default() };
        let mut input = json!({"state": "all", "projectId": project, "limit": 99});
        if !key.1.is_empty() {
            input["query"] = json!(key.1);
        }
        match s.src.net() {
            Some(net) => {
                let request = net.request("pullRequests.list", input);
                let prs = &mut s.ui.chat.composer.mention.prs;
                prs.request = Some(request);
                prs.loading = true;
            }
            None if !crate::host::live() => {
                let entries = crate::pages::fixture("pull-requests.json").and_then(|v| v["entries"].as_array().cloned()).unwrap_or_default();
                s.ui.chat.composer.mention.prs.entries = entries.into_iter().map(|mut e| {
                    e["projectId"] = json!(project);
                    e
                }).collect();
            }
            None => {}
        }
        return;
    }
    if m.kind != Kind::Path || s.ui.chat.composer.mention.query.as_deref() == Some(m.query()) {
        return;
    }
    let query = m.query().to_string();
    let root = search_root(s);
    let mention = &mut s.ui.chat.composer.mention;
    mention.query = Some(query.clone());
    mention.entries.clear();
    mention.request = None;
    mention.loading = false;
    let (Some(root), Some(net)) = (root, s.src.net()) else { return };
    // A bare `@` lists nothing until a character follows (useComposerMenuItems).
    if query.is_empty() {
        return;
    }
    let request = net.request("projects.searchEntries", json!({"cwd": root, "query": query, "limit": 80}));
    let mention = &mut s.ui.chat.composer.mention;
    mention.request = Some(request);
    mention.loading = true;
}

pub fn poll(mention: &mut Mention, net: &mut s5_net::Net) -> bool {
    let prs = &mut mention.prs;
    let pr_reply = prs.request.and_then(|r| net.take_reply(r));
    let got_prs = pr_reply.is_some();
    if let Some(reply) = pr_reply {
        prs.request = None;
        prs.loading = false;
        match reply {
            Ok(v) => prs.entries = v["entries"].as_array().cloned().unwrap_or_default(),
            Err(_) => prs.failed = true,
        }
    }
    let Some(request) = mention.request else { return got_prs };
    let Some(reply) = net.take_reply(request) else { return false };
    mention.request = None;
    mention.loading = false;
    mention.entries = reply
        .ok()
        .and_then(|v| v["entries"].as_array().cloned())
        .unwrap_or_default()
        .iter()
        .map(|e| (data::s(e, "path").to_string(), data::s(e, "kind") == "directory"))
        .collect();
    true
}

pub fn choose(s: &mut App, m: &Open, index: usize, _tx: &mut Tx) {
    let Some(item) = items(s, m).into_iter().nth(index) else { return };
    s.ui.chat.composer.menu_active = 0;
    if item.id == "/model" {
        let d = draft_mut(s);
        d.edit.set_selection(m.start, m.end);
        d.edit.insert("");
        super::picker::toggle(s);
        return;
    }
    if let Some(mode) = item.id.strip_prefix('/').filter(|_| m.kind == Kind::Slash).filter(|id| PLAN_SLASH.iter().any(|(n, _)| n == id)) {
        let d = draft_mut(s);
        d.edit.set_selection(m.start, m.end);
        d.edit.insert("");
        set_interaction_mode(s, mode);
        return;
    }
    if let Some(pr) = &item.pr {
        let record = super::chips::pull_request(pr, false, &[]);
        let d = draft_mut(s);
        d.edit.replace_with_pill(m.start..m.end, super::chips::pill_id(&record), data::s(&record, "label"));
        d.edit.insert(" ");
        d.contexts.push(record);
        return;
    }
    let d = draft_mut(s);
    d.edit.set_selection(m.start, m.end);
    match m.kind {
        Kind::Path => {
            d.edit.replace_with_pill(m.start..m.end, item.id.clone(), &item.label);
            d.edit.insert(" ");
        }
        _ => d.edit.insert(&format!("{} ", item.id)),
    }
}

fn surface(t: &T) -> El {
    col()
        .key("composer-menu")
        .corners([radius::XL2, radius::XL2, 0.0, 0.0])
        .fill(if t.dark { t.c.surface_raised } else { t.c.card })
        .border(1.0, if t.dark { rune::Color::WHITE.alpha(0.05) } else { rune::Color::BLACK.alpha(0.08) })
}

pub fn view(s: &App, _cx: &mut Cx<App>, t: &T) -> Option<El> {
    if s.ui.chat.composer.picker.open {
        return None;
    }
    let m = open_menu(s)?;
    let list = items(s, &m);
    if list.is_empty() {
        let prs = &s.ui.chat.composer.mention.prs;
        let message = match m.kind {
            Kind::Path if s.ui.chat.composer.mention.loading => "Searching workspace files...".to_string(),
            Kind::Path => "No matching files or folders.".to_string(),
            Kind::Skill => "No skills found. Try / to browse provider commands.".to_string(),
            Kind::Slash => "No matching command.".to_string(),
            Kind::PullRequest if ctx(s).project.is_none() => "Pull requests are not available for this project.".to_string(),
            Kind::PullRequest if prs.loading || prs.key.is_none() => "Finding pull request...".to_string(),
            Kind::PullRequest if prs.failed => "Pull requests could not be read for this project.".to_string(),
            Kind::PullRequest if !m.query().is_empty() => format!("No pull request matches {}.", m.query()),
            Kind::PullRequest => "No pull requests found in this repository.".to_string(),
        };
        return Some(surface(t).px(20.0).pt(14.0).pb(28.0 + 17.0).child(text(message).font(theme::TEXT_XS).color(t.c.secondary_label)));
    }
    let active = s.ui.chat.composer.menu_active.min(list.len() - 1);
    let mut rows = col().w_full();
    for (i, item) in list.into_iter().enumerate() {
        let mut r = row()
            .key(format!("composer-menu-{}", item.id))
            .h(28.0)
            .px(8.0)
            .gap(8.0)
            .center_y()
            .radius(theme::layout::CONTROL_RADIUS)
            .cursor(Cursor::Pointer)
            .on_hover(move |s: &mut App| s.ui.chat.composer.menu_active = i)
            .on_press_tx(move |s: &mut App, tx| {
                if let Some(m) = open_menu(s) {
                    choose(s, &m, i, tx);
                }
            });
        if i == active {
            r = r.fill(t.c.accent);
        }
        if let Some(folder) = item.folder {
            r = r.child(icon(if folder { &icons::FOLDER } else { &icons::FILE }, 16.0).color(t.c.secondary_label));
        }
        if let Some(pr) = &item.pr
            && let Some((tone, glyph)) = super::chips::look("review-comment", Some(&json!({"pullRequest": pr})))
        {
            r = r.child(icon(glyph, 16.0).color(tone));
        }
        r = r
            .child(text(item.label).font(theme::XS_MEDIUM).color(t.c.foreground).truncate().shrink())
            .child(text(item.description).font(theme::TEXT_XS).color(t.c.secondary_label).truncate().grow(1.0).shrink().min_w(0.0));
        rows = rows.child(r);
    }
    let rows = col().w_full().max_h(280.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(rows);
    Some(surface(t).px(8.0).pt(8.0).pb(8.0 + 17.0).child(rows).child(el()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn slash_labels(model: &str) -> Vec<String> {
        let mut s = App::new();
        let d = draft_mut(&mut s);
        d.model = Some((model.into(), "m".into()));
        d.edit = rune::TextEdit::new("/");
        d.edit.set_caret(1);
        let m = open_menu(&s).unwrap();
        items(&s, &m).into_iter().map(|i| i.label).take(5).collect()
    }

    #[test]
    fn slash_commands_rank_by_name_then_description_and_mid_prompt_keep_built_ins() {
        let it = |n: &str, d: &str| Item { id: format!("/{n}"), label: format!("/{n}"), description: d.into(), folder: None, pr: None };
        let labels = |items: Vec<Item>| items.into_iter().map(|i| i.label).collect::<Vec<_>>();
        let list = vec![it("model", "Switch response model"), it("side", "Ask a side question"), it("review", "Review the diff"), it("mode", "x")];
        assert_eq!(labels(ranked_slash(list, 2, "mod")), ["/mode", "/model"]);
        let list = vec![it("model", "Switch response model"), it("review", "Review the side diff")];
        assert_eq!(labels(ranked_slash(list, 1, "side")), ["/review"]);
        let mut s = App::new();
        let d = draft_mut(&mut s);
        d.model = Some(("codex".into(), "m".into()));
        d.edit = rune::TextEdit::new("fix it /");
        d.edit.set_caret(8);
        let m = open_menu(&s).unwrap();
        assert_eq!(m.kind, Kind::Slash);
        assert_eq!(labels(items(&s, &m)), ["/model", "/plan", "/default"]);
        let d = draft_mut(&mut s);
        d.edit = rune::TextEdit::new("/usr/bin");
        d.edit.set_caret(1);
        assert!(open_menu(&s).is_none());
    }

    #[test]
    fn a_hash_opens_pull_requests_only_before_a_name_like_word() {
        let kind = |text: &str| {
            let mut s = App::new();
            let d = draft_mut(&mut s);
            d.edit = rune::TextEdit::new(text);
            d.edit.set_caret(text.len());
            open_menu(&s).map(|m| m.kind)
        };
        assert_eq!(kind("see #"), Some(Kind::PullRequest));
        assert_eq!(kind("see #42"), Some(Kind::PullRequest));
        assert_eq!(kind("#fix-drafts_2"), Some(Kind::PullRequest));
        assert_eq!(kind("#-x"), None);
        assert_eq!(kind("#a.b"), None);
    }

    #[test]
    fn plan_commands_follow_model_only_for_plan_capable_providers() {
        assert_eq!(slash_labels("codex")[..3], ["/model", "/plan", "/default"]);
        assert_eq!(slash_labels("devReplay")[..1], ["/model"]);
        assert!(!slash_labels("codex").iter().any(|l| l == "/side" || l == "/btw"), "side chats are not implemented");
    }

    #[test]
    fn slash_lists_skills_and_offers_compact_only_alone() {
        let labels = |model: &str, text: &str| {
            let mut s = App::new();
            let d = draft_mut(&mut s);
            d.model = Some((model.into(), "m".into()));
            d.edit = rune::TextEdit::new(text);
            d.edit.set_caret(1);
            let m = open_menu(&s).unwrap();
            items(&s, &m).into_iter().map(|i| i.label).collect::<Vec<_>>()
        };
        assert!(labels("codex", "/").contains(&"/compact".to_owned()));
        assert!(!labels("codex", "/ and more").contains(&"/compact".to_owned()));
        assert!(labels("opencode", "/").contains(&"/skill:contribution-triage".to_owned()));
    }
}
