//! Tool-call groups (work-log presentation.ts): consecutive tool calls in one run fold into a
//! summary row ("Read 4 files and ran 2 commands") that expands; a running group names its
//! latest step instead. Also the live "Working" and "Thinking" rows, superseded attempts and the
//! steer marker on user messages.
use super::rows::{self, array, tool_token};
use super::{item_view, toggle};
use crate::data::s;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, ScrollSpec, col, el, icon, row, text};
use serde_json::Value;
use std::collections::HashSet;

/// `ToolGroupAction`, for the kinds this client sees.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Action {
    Read,
    Edit,
    Command,
    ThreadCreate,
    Search,
    CodeSearch,
    Other,
    Update,
}

pub use super::rows::{groupable, item_active as active};

/// `workEntryDisplayIndicatesToolFailure`, from the fields the wire keeps.
pub fn failed(item: &Value) -> bool {
    matches!(s(item, "status"), "failed" | "declined") || item["exitCode"].as_i64().is_some_and(|c| c != 0) || item["outputIndicatesFailure"] == true
}

/// `toolGroupAction` + `classifyToolActivity`.
pub fn action(item: &Value) -> Action {
    match s(item, "type") {
        "thread_created" => Action::ThreadCreate,
        "command_execution" => Action::Command,
        "file_change" => Action::Edit,
        "file_search" => Action::CodeSearch,
        "web_search" => Action::Search,
        "approval_request" if s(item, "requestKind") == "file-read" => Action::Read,
        "approval_request" | "user_input_request" => Action::Other,
        "dynamic_tool" if !item["viewedImagePath"].is_null() => Action::Read,
        "dynamic_tool" => match tool_token(s(item, "toolName")).as_deref() {
            Some("find" | "grep" | "glob" | "rg" | "ls") => Action::CodeSearch,
            Some("terminal" | "bash" | "shell") => Action::Command,
            Some("read" | "readfile") => Action::Read,
            _ => Action::Other,
        },
        _ => Action::Update,
    }
}

fn priority(action: Action) -> u8 {
    match action {
        Action::Command | Action::Edit | Action::ThreadCreate => 0,
        Action::Other | Action::Update => 2,
        _ => 1,
    }
}

fn label(action: Action, n: usize) -> String {
    let p = |one: &'static str, many: &'static str| if n == 1 { one } else { many };
    match action {
        Action::Read => format!("Read {n} {}", p("file", "files")),
        Action::Edit => format!("Changed {n} {}", p("file", "files")),
        Action::Command => format!("Ran {n} {}", p("command", "commands")),
        Action::ThreadCreate => format!("Created {n} {}", p("thread", "threads")),
        Action::Search => format!("Searched the web {n} {}", p("time", "times")),
        Action::CodeSearch => format!("Searched code {n} {}", p("time", "times")),
        Action::Other => format!("Used {n} {}", p("tool", "tools")),
        Action::Update => format!("Received {n} {}", p("update", "updates")),
    }
}

/// `toolGroupActionCount` for edits: distinct changed paths, plus edits that name none.
fn edit_count(items: &[&Value]) -> usize {
    let mut paths = HashSet::new();
    let mut bare = 0;
    for item in items {
        let mut named: Vec<&str> = array(item, "changes").iter().filter_map(|c| c["path"].as_str()).filter(|p| !p.is_empty()).collect();
        if named.is_empty() {
            named.extend(item["fileName"].as_str().filter(|p| !p.is_empty()));
        }
        if named.is_empty() {
            bare += 1;
        }
        paths.extend(named);
    }
    paths.len() + bare
}

fn lower_first(label: &str) -> String {
    let mut chars = label.chars();
    match chars.next() {
        Some(first) => first.to_lowercase().chain(chars).collect(),
        None => String::new(),
    }
}

/// `summarizeToolGroup`: the two leading actions (commands and edits first), then a count of the rest.
pub fn summary(items: &[&Value]) -> String {
    let tools: Vec<&Value> = items.iter().copied().filter(|i| s(i, "type") != "reasoning").collect();
    if tools.is_empty() {
        return if items.len() == 1 { "Thought".into() } else { format!("Thought (×{})", items.len()) };
    }
    let mut groups: Vec<(Action, Vec<&Value>)> = Vec::new();
    for &item in &tools {
        let kind = action(item);
        match groups.iter_mut().find(|(g, _)| *g == kind) {
            Some((_, list)) => list.push(item),
            None => groups.push((kind, vec![item])),
        }
    }
    let mut picked: Vec<usize> = (0..groups.len()).collect();
    picked.sort_by_key(|&i| (priority(groups[i].0), i));
    picked.truncate(2);
    picked.sort_unstable();
    let mut labels: Vec<String> = picked
        .iter()
        .map(|&i| {
            let (kind, list) = &groups[i];
            label(*kind, if *kind == Action::Edit { edit_count(list) } else { list.len() })
        })
        .collect();
    let rest = tools.len() - picked.iter().map(|&i| groups[i].1.len()).sum::<usize>();
    if rest > 0 {
        labels.push(format!("Performed {rest} other {}", if rest == 1 { "action" } else { "actions" }));
    }
    let labels: Vec<String> = labels.into_iter().enumerate().map(|(i, l)| if i == 0 { l } else { lower_first(&l) }).collect();
    match labels.len() {
        0..=2 => labels.join(" and "),
        n => format!("{}, and {}", labels[..n - 1].join(", "), labels[n - 1]),
    }
}

/// A group's icon by what it holds (`toolGroupSummaryKind`).
fn group_icon(items: &[&Value]) -> &'static rune::Icon {
    let tools: Vec<&Value> = items.iter().copied().filter(|i| s(i, "type") != "reasoning").collect();
    let Some(first) = tools.first() else { return &icons::BRAIN };
    let kind = action(first);
    if tools.iter().any(|i| action(i) != kind) {
        return &icons::HAMMER;
    }
    match kind {
        Action::Other if tools.iter().all(|i| s(i, "type") == "dynamic_tool") => &icons::HAMMER,
        other => action_icon(other),
    }
}

fn action_icon(action: Action) -> &'static rune::Icon {
    match action {
        Action::Read => &icons::EYE,
        Action::Edit => &icons::SQUARE_PEN,
        Action::Command => &icons::TERMINAL,
        Action::Search => &icons::GLOBE,
        Action::CodeSearch => &icons::SEARCH,
        Action::ThreadCreate => &super::T3_MARK,
        Action::Other => &icons::WRENCH,
        Action::Update => &icons::HAMMER,
    }
}

/// `workEntryIconName`: what an entry's own row shows, by kind, surface and tool.
pub fn entry_icon(item: &Value) -> &'static rune::Icon {
    match s(item, "type") {
        "approval_request" | "user_input_request" => return &icons::MESSAGE_CIRCLE,
        "todo_list" => return &icons::LIST_TODO,
        "reasoning" => return &icons::BRAIN,
        "subagent" => return &icons::BOT,
        _ => {}
    }
    match s(item, "toolSurface") {
        "browser" => return &icons::PANELS_TOP_LEFT,
        "computer" => return &icons::MONITOR,
        _ => {}
    }
    if matches!(s(item, "toolName"), "js" | "cua_repl.js" | "node_repl.js") {
        return &icons::CODE;
    }
    match action(item) {
        Action::Other if s(item, "type") != "dynamic_tool" => &icons::ZAP,
        other => action_icon(other),
    }
}

/// `liveThoughtLine`: a reasoning block's bold heading, else its first sentence, as plain text.
pub fn thought_line(markdown: &str) -> String {
    let first = markdown.trim_start().lines().next().unwrap_or("").trim_end();
    let heading = first.strip_prefix("**").and_then(|r| r.strip_suffix("**")).filter(|h| !h.is_empty() && !h.contains('*'));
    let source = heading.unwrap_or(markdown);
    let joined = source.lines().map(|line| strip_marker(line.trim_start()).unwrap_or(line)).collect::<Vec<_>>().join(" ");
    let plain = strip_links(&joined).replace("**", "").replace("~~", "").replace('`', "");
    let text = plain.split_whitespace().collect::<Vec<_>>().join(" ");
    if heading.is_some() {
        return text;
    }
    let chars: Vec<(usize, char)> = text.char_indices().collect();
    for (k, &(_, c)) in chars.iter().enumerate() {
        if !matches!(c, '.' | '?' | '!') {
            continue;
        }
        let mut end = k + 1;
        if chars.get(end).is_some_and(|&(_, q)| matches!(q, '"' | '\'' | '”' | '’' | ')')) {
            end += 1;
        }
        if let Some(&(at, ' ')) = chars.get(end) {
            return text[..at].to_owned();
        }
    }
    text
}

/// A heading, bullet or numbered-list marker and the space after it.
fn strip_marker(line: &str) -> Option<&str> {
    let hashes = line.len() - line.trim_start_matches('#').len();
    let rest = if (1..=6).contains(&hashes) {
        &line[hashes..]
    } else if let Some(rest) = line.strip_prefix(['-', '*', '+']) {
        rest
    } else {
        let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
        if digits == 0 {
            return None;
        }
        line[digits..].strip_prefix('.')?
    };
    rest.starts_with([' ', '\t']).then(|| rest.trim_start())
}

/// `[label](url)` and `![alt](url)` as their label.
fn strip_links(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let after = &rest[open + 1..];
        let parsed = after.find(']').and_then(move |close| {
            let tail = after[close + 1..].strip_prefix('(')?;
            let end = tail.find(')')?;
            Some((&after[..close], &tail[end + 1..]))
        });
        match parsed {
            Some((label, tail)) => {
                out.push_str(rest[..open].strip_suffix('!').unwrap_or(&rest[..open]));
                out.push_str(label);
                rest = tail;
            }
            None => {
                out.push_str(&rest[..=open]);
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Shell setup a command line runs before the program it names (`cd x && vp test`).
const SETUP: [&str; 12] = ["cd", "pushd", "popd", "source", ".", "export", "set", "unset", "alias", "setopt", "shopt", "umask"];

/// commandLabel.ts `commandProgramName`, for the usual shapes: comments, setup steps,
/// environment assignments and wrappers (`sudo -u root`, `env`, ...) skipped, `bash -lc "..."`
/// unwrapped, the program's file name.
pub fn program(command: &str) -> Option<String> {
    program_in(command, 0)
}

fn program_in(script: &str, depth: u8) -> Option<String> {
    let line = script.lines().map(str::trim).find(|l| !l.is_empty() && !l.starts_with('#'))?;
    for words in segments(line).into_iter().take(8) {
        match segment_program(&words, depth) {
            Some(name) if SETUP.contains(&name.as_str()) => continue,
            found => return found,
        }
    }
    None
}

/// A line's words, quotes removed, split into commands at unquoted `;`, `&` and `|`.
fn segments(line: &str) -> Vec<Vec<String>> {
    let (mut out, mut words, mut word) = (Vec::new(), Vec::new(), String::new());
    let (mut quote, mut escaped, mut started) = (None, false, false);
    for c in line.chars() {
        match (quote, c) {
            _ if escaped => {
                word.push(c);
                escaped = false;
            }
            (Some('\''), '\'') | (Some('"'), '"') => quote = None,
            (Some('"'), '\\') => escaped = true,
            (Some(_), _) => word.push(c),
            (None, '\'' | '"') => {
                quote = Some(c);
                started = true;
            }
            (None, c) if c.is_whitespace() || matches!(c, ';' | '&' | '|') => {
                if started {
                    words.push(std::mem::take(&mut word));
                    started = false;
                }
                if !c.is_whitespace() && !words.is_empty() {
                    out.push(std::mem::take(&mut words));
                }
            }
            (None, c) => {
                word.push(c);
                started = true;
            }
        }
    }
    if started {
        words.push(word);
    }
    if !words.is_empty() {
        out.push(words);
    }
    out
}

fn segment_program(words: &[String], depth: u8) -> Option<String> {
    let mut at = 0;
    let mut wrapped = false;
    while let Some(word) = words.get(at) {
        let assignment = word.split_once('=').is_some_and(|(name, _)| {
            !name.is_empty() && !name.starts_with(|c: char| c.is_ascii_digit()) && name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
        });
        let wrapper = matches!(word.as_str(), "sudo" | "env" | "time" | "nohup" | "exec" | "command" | "builtin");
        if !(assignment || wrapper || (wrapped && word.starts_with('-'))) {
            break;
        }
        // A wrapper's option that takes a value: `sudo -u root`, `env -u DEBUG`.
        let takes_value = wrapped && matches!(word.as_str(), "-u" | "-g" | "-C" | "-D" | "-p" | "-U" | "-h");
        wrapped |= wrapper;
        at += if takes_value { 2 } else { 1 };
    }
    let path = words.get(at)?;
    let file = path.rsplit(|c: char| c == '/' || c == '\\').next().unwrap_or(path);
    let name = file.strip_suffix(".exe").or_else(|| file.strip_suffix(".EXE")).unwrap_or(file);
    if name.is_empty() {
        return None;
    }
    if depth < 3 && matches!(name, "bash" | "sh" | "zsh" | "dash" | "ksh" | "fish") {
        let flag = words[at + 1..].iter().position(|w| w.starts_with('-') && !w.starts_with("--") && w.contains('c'));
        if let Some(flag) = flag {
            return words.get(at + 2 + flag).and_then(|script| program_in(script, depth + 1));
        }
    }
    Some(name.to_owned())
}

/// What a running group's header says (`liveWorkEntryLabel`): its latest step.
pub fn live_label(item: &Value, root: Option<&str>) -> String {
    match s(item, "type") {
        "reasoning" => {
            let line = thought_line(s(item, "text"));
            if !line.is_empty() {
                line
            } else if active(item) {
                "Thinking".into()
            } else {
                "Thought".into()
            }
        }
        "command_execution" => {
            let verb = match s(item, "status") {
                "failed" => "Failed",
                "declined" => "Declined",
                "cancelled" | "interrupted" => "Stopped",
                _ if active(item) => "Running",
                _ => "Ran",
            };
            format!("{verb} {}", program(s(item, "input")).unwrap_or_else(|| "command".to_owned()))
        }
        _ => rows::entry_label(item, root),
    }
}

/// A finished turn's fold content: runs of two or more groupable items in one run become groups.
pub enum Entry<'a> {
    One(&'a Value),
    Group(String, Vec<&'a Value>),
}

pub fn group<'a>(items: &[&'a Value]) -> Vec<Entry<'a>> {
    fn flush<'b>(run: &mut Vec<&'b Value>, out: &mut Vec<Entry<'b>>) {
        match run.len() {
            0 => {}
            1 => out.push(Entry::One(run[0])),
            _ => out.push(Entry::Group(format!("tools:{}", s(run[0], "id")), run.clone())),
        }
        run.clear();
    }
    let mut out = Vec::new();
    let mut run: Vec<&'a Value> = Vec::new();
    for &item in items {
        let joins = groupable(item) && run.last().is_none_or(|prev| s(prev, "runId") == s(item, "runId"));
        if !joins {
            flush(&mut run, &mut out);
        }
        if groupable(item) {
            run.push(item);
        } else {
            out.push(Entry::One(item));
        }
    }
    flush(&mut run, &mut out);
    out
}

/// A tool group's header; open, the calls it holds, each its own expandable row, in a box that
/// scrolls past min(18rem, half the timeline) plus what its open rows add
/// (ExpandedWorkGroupEntries).
pub fn group_view(app: &App, id: &str, items: &[&Value], t: &T) -> El {
    let expanded = app.ui.chat.timeline.expanded.contains(id);
    let root = super::workspace_root(app);
    let last = items.last().copied();
    let live = last.is_some_and(active);
    let (title, glyph) = match last {
        Some(item) if live => (live_label(item, root.as_deref()), entry_icon(item)),
        _ => (summary(items), group_icon(items)),
    };
    // hasFailure: only the latest tool call's outcome marks the group.
    let broke = items.iter().rev().find(|i| s(i, "type") != "reasoning").is_some_and(|i| failed(i));
    let color = if broke { t.c.destructive.alpha_mul(0.4) } else { t.c.muted_foreground };
    let spoken = if broke { format!("{title}, tool call failed") } else { title.clone() };
    let head_key = format!("group-head-{id}");
    let toggle_id = id.to_owned();
    let head = row()
        .key(head_key.clone())
        .w_full()
        .gap(8.0)
        .py(4.0)
        .px(4.0)
        .radius(6.0)
        .hover(t.c.muted.alpha_mul(0.4))
        .focusable(true)
        .role("button")
        .label(spoken)
        .expanded(expanded)
        .on_press_tx(move |app: &mut App, tx| {
            tx.keep_on_screen(head_key.clone());
            toggle(app, &toggle_id)
        })
        .child(icon(glyph, 16.0).color(color))
        .child(text(title).font(theme::TEXT_SM).color(t.c.muted_foreground).truncate().grow(1.0).min_w(0.0));
    let mut out = col().w_full().gap(4.0).child(head);
    if expanded {
        let open = &app.ui.chat.timeline.expanded;
        let mut list = col().w_full();
        let mut grown = 0.0;
        for item in items {
            let item_id = s(item, "id");
            let entry_key = format!("group-entry-{item_id}");
            if open.contains(item_id) {
                let full = rune::memo::layout_with(&entry_key, |r| r[3]);
                let compact = rune::memo::layout_with(&format!("item-head-{item_id}"), |r| r[3]);
                grown += full.zip(compact).map_or(0.0, |(full, compact)| (full - compact).max(0.0));
            }
            list = list.child(col().key(entry_key).w_full().child(item_view(app, item, t)));
        }
        let cap = match app.frame_route() {
            crate::Route::Thread(thread) => rune::memo::layout_with(&format!("timeline-{thread}"), |r| (r[3] * 0.5).min(288.0).round()),
            _ => None,
        };
        let viewport = list.scroll(ScrollSpec { stick_bottom: live });
        out = out.child(crate::kit::scroll_area(&format!("group-scroll-{id}"), viewport, t).max_h(cap.unwrap_or(288.0) + grown));
    }
    out
}

/// AttemptFoldTimelineRow: what an attempt a steer cut off left behind; open, its work below.
pub fn attempt_view(app: &App, id: &str, items: &[&Value], t: &T) -> El {
    let expanded = app.ui.chat.timeline.expanded.contains(id);
    let head_key = format!("attempt-head-{id}");
    let toggle_id = id.to_owned();
    let head = row()
        .key(head_key.clone())
        .w_full()
        .gap(8.0)
        .px(10.0)
        .py(8.0)
        .center_y()
        .radius(theme::radius::MD)
        .border(1.0, t.c.border.alpha_mul(0.6))
        .fill(t.c.muted.alpha_mul(0.2))
        .hover(t.c.muted.alpha_mul(0.35))
        .focusable(true)
        .role("button")
        .label("Superseded attempt, cut off by a steer")
        .expanded(expanded)
        .on_press_tx(move |app: &mut App, tx| {
            tx.keep_on_screen(head_key.clone());
            toggle(app, &toggle_id)
        })
        .child(icon(if expanded { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 14.0).color(t.c.muted_foreground))
        .child(text("Superseded attempt").font(theme::XS_MEDIUM).color(t.c.foreground.alpha_mul(0.8)))
        .child(text("Cut off by a steer").font(theme::TEXT_2XS).color(t.c.muted_foreground));
    let mut out = col().w_full().gap(4.0).child(head);
    if expanded {
        for entry in group(items) {
            out = out.child(match entry {
                Entry::One(item) => item_view(app, item, t),
                Entry::Group(group_id, members) => group_view(app, &group_id, &members, t),
            });
        }
    }
    out
}

/// WorkingTimelineRow: the turn in flight's header; compacting, behind a minimize glyph.
pub fn working_view(label: String, compacting: bool, t: &T) -> El {
    let mut line = row().h(24.0).px(4.0).gap(6.0).center_y();
    if compacting {
        line = line.child(icon(&icons::MINIMIZE_2, 12.0).color(t.c.muted_foreground));
    }
    col()
        .w_full()
        .pt(4.0)
        .pb(8.0)
        .child(line.child(text(label).font(theme::TEXT_SM.lh(22.75)).color(t.c.muted_foreground)))
        .child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6)))
}

/// The running turn's "Thinking" row while nothing else shows activity.
pub fn thinking_view(t: &T) -> El {
    row()
        .w_full()
        .gap(8.0)
        .py(4.0)
        .px(4.0)
        .center_y()
        .label("Thinking")
        .child(icon(&icons::BRAIN, 16.0).color(t.c.muted_foreground))
        .child(text("Thinking").font(theme::TEXT_SM).color(t.c.muted_foreground))
}

/// UserMessageIntentMarker's tooltip for a message that steered a running turn.
pub fn steer(item: &Value) -> Option<&'static str> {
    match s(item, "inputIntent") {
        "steer" => Some("Steered the active turn"),
        "promoted_queued_to_steer" => Some("Originally queued, then promoted to steer the active turn"),
        _ => None,
    }
}

pub fn steer_marker(help: &'static str, t: &T) -> El {
    row()
        .gap(4.0)
        .center_y()
        .help(help)
        .child(icon(&icons::REDO_2, 12.0).color(t.c.muted_foreground))
        .child(text("Steer").font(theme::TEXT_XS).color(t.c.muted_foreground))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn summarize(items: &[Value]) -> String {
        summary(&items.iter().collect::<Vec<_>>())
    }

    #[test]
    fn summaries_lead_with_commands_and_edits_and_count_the_rest() {
        let read = |p: &str| json!({"type":"dynamic_tool","toolName":"Read","input":{"path":p}});
        let cmd = json!({"type":"command_execution","input":"ls"});
        let edit = |p: &str| json!({"type":"file_change","changes":[{"path":p}]});
        assert_eq!(summarize(&[read("a"), read("b")]), "Read 2 files");
        assert_eq!(summarize(&[read("a"), cmd.clone(), cmd.clone()]), "Read 1 file and ran 2 commands");
        let search = json!({"type":"file_search","pattern":"x"});
        assert_eq!(summarize(&[read("a"), read("b"), cmd.clone(), search]), "Read 2 files, ran 1 command, and performed 1 other action");
        // Edits count distinct files; reasoning is left out of a mixed group.
        let thought = json!({"type":"reasoning","text":"hm"});
        assert_eq!(summarize(&[edit("a"), thought.clone(), edit("a"), edit("b")]), "Changed 2 files");
        assert_eq!(summarize(&[thought.clone()]), "Thought");
        assert_eq!(summarize(&[thought.clone(), thought]), "Thought (×2)");
        let mcp = json!({"type":"dynamic_tool","toolName":"mcp__t3__thread_read"});
        let web = json!({"type":"web_search"});
        // Tools rank after searches but keep their order of first appearance.
        assert_eq!(summarize(&[mcp.clone(), web, mcp]), "Used 2 tools and searched the web 1 time");
    }

    #[test]
    fn tools_classify_by_kind_and_tool_name() {
        assert_eq!(action(&json!({"type":"dynamic_tool","toolName":"Read_File"})), Action::Read);
        assert_eq!(action(&json!({"type":"dynamic_tool","toolName":"Grep"})), Action::CodeSearch);
        assert_eq!(action(&json!({"type":"dynamic_tool","toolName":"Bash"})), Action::Command);
        assert_eq!(action(&json!({"type":"dynamic_tool","toolName":"view","viewedImagePath":"/a.png"})), Action::Read);
        assert_eq!(action(&json!({"type":"dynamic_tool","toolName":"fs.read"})), Action::Other);
        assert_eq!(action(&json!({"type":"approval_request","requestKind":"file-read"})), Action::Read);
        assert_eq!(action(&json!({"type":"notification"})), Action::Update);
    }

    #[test]
    fn folds_group_consecutive_tools_of_one_run() {
        let items = [
            json!({"id":"a","type":"command_execution","runId":"r"}),
            json!({"id":"b","type":"reasoning","runId":"r"}),
            json!({"id":"m","type":"assistant_message","runId":"r"}),
            json!({"id":"c","type":"command_execution","runId":"r"}),
            json!({"id":"d","type":"file_change","runId":"r"}),
            json!({"id":"e","type":"file_change","runId":"s"}),
        ];
        let refs: Vec<&Value> = items.iter().collect();
        let shape: Vec<String> = group(&refs)
            .into_iter()
            .map(|e| match e {
                Entry::One(i) => s(i, "id").to_owned(),
                Entry::Group(id, items) => format!("{id}[{}]", items.len()),
            })
            .collect();
        assert_eq!(shape, ["tools:a[2]", "m", "tools:c[2]", "e"]);
    }

    #[test]
    fn thought_lines_take_the_heading_or_first_sentence() {
        assert_eq!(thought_line("**Planning the fix**\n\nI will read `a.rs`. Then edit."), "Planning the fix");
        assert_eq!(thought_line("I will read [the file](x.rs) first. Then edit it."), "I will read the file first.");
        assert_eq!(thought_line("- Check **all** callers\n- then fix"), "Check all callers then fix");
        assert_eq!(thought_line("Done?\" yes"), "Done?\"");
        assert_eq!(thought_line("v1.2 is out"), "v1.2 is out");
    }

    #[test]
    fn live_groups_name_their_latest_step() {
        let live = |item: Value| live_label(&item, None);
        assert_eq!(live(json!({"type":"command_execution","status":"running","input":"vp test run\n--watch"})), "Running vp");
        assert_eq!(live(json!({"type":"command_execution","status":"completed","input":"ls -la"})), "Ran ls");
        assert_eq!(live(json!({"type":"command_execution","status":"failed","input":"cargo test"})), "Failed cargo");
        assert_eq!(live(json!({"type":"command_execution","status":"interrupted","input":""})), "Stopped command");
        assert_eq!(live(json!({"type":"reasoning","status":"running","text":""})), "Thinking");
        assert_eq!(live(json!({"type":"file_search","title":"Searched src"})), "Searched src");
        assert_eq!(live_label(&json!({"type":"dynamic_tool","toolName":"Read","input":{"path":"/w/app/a.rs"}}), Some("/w/app")), "Read app/a.rs");
    }

    #[test]
    fn commands_are_named_by_their_program() {
        assert_eq!(program("FOO=1 sudo -E /usr/bin/git status").as_deref(), Some("git"));
        assert_eq!(program("bash -lc \"cd src && rg derive\"").as_deref(), Some("rg"));
        assert_eq!(program("zsh -lc 'CI=1 env -u DEBUG sudo -u root vp test run'").as_deref(), Some("vp"));
        assert_eq!(program("/bin/bash --rcfile /tmp/config -c 'git status'").as_deref(), Some("git"));
        assert_eq!(program("# setup\ngit status").as_deref(), Some("git"));
        assert_eq!(program("zsh -lc 'env'"), None);
        assert_eq!(program("/bin/zsh -c 'pnpm test'").as_deref(), Some("pnpm"));
        assert_eq!(program("C:\\tools\\node.exe build.js").as_deref(), Some("node"));
        assert_eq!(program("  "), None);
    }

    #[test]
    fn steers_say_how_they_reached_the_turn() {
        assert_eq!(steer(&json!({"inputIntent":"steer"})), Some("Steered the active turn"));
        assert!(steer(&json!({"inputIntent":"promoted_queued_to_steer"})).is_some_and(|h| h.starts_with("Originally queued")));
        assert_eq!(steer(&json!({"inputIntent":"turn_start"})), None);
    }
}
