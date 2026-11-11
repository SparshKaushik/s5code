//! Tool-call groups (work-log presentation.ts): consecutive tool calls in one run fold into a
//! summary row ("Read 4 files and ran 2 commands") that expands; a running group names its
//! latest step instead. Also the live "Thinking" row and the steer marker on user messages.
use super::rows::{self, array};
use super::{item_view, toggle};
use crate::data::s;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, col, icon, row, text};
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

/// toolActivity.ts `toolNameToken`: namespaced names (`mcp__x`, `a.b`, `a/b`) classify as nothing.
fn tool_token(name: &str) -> Option<String> {
    let name = name.trim();
    if name.is_empty() || name.contains("__") || name.contains(['.', '/']) {
        return None;
    }
    Some(name.chars().filter(|c| !matches!(c, '_' | '-') && !c.is_whitespace()).collect::<String>().to_lowercase())
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
        Action::ThreadCreate => &icons::MESSAGES_SQUARE,
        Action::Other => &icons::WRENCH,
        Action::Update => &icons::HAMMER,
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

/// What a running group's header says: its latest step.
pub fn live_label(item: &Value) -> String {
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
            let command = s(item, "input").lines().next().unwrap_or("").trim();
            let verb = if active(item) { "Running" } else { "Ran" };
            if command.is_empty() { format!("{verb} command") } else { format!("{verb} {command}") }
        }
        _ => rows::label(item),
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

/// A tool group's header; open, the calls it holds, each its own expandable row.
pub fn group_view(app: &App, id: &str, items: &[&Value], t: &T) -> El {
    let expanded = app.ui.chat.timeline.expanded.contains(id);
    let last = items.last().copied();
    let live = last.is_some_and(active);
    let (title, glyph) = match last {
        Some(item) if live => (live_label(item), if s(item, "type") == "reasoning" { &icons::BRAIN } else { action_icon(action(item)) }),
        _ => (summary(items), group_icon(items)),
    };
    let broke = !live && items.iter().any(|i| failed(i));
    let color = if broke { t.c.destructive.alpha_mul(0.4) } else { t.c.muted_foreground };
    let spoken = if broke { format!("{title}, a tool call failed") } else { title.clone() };
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
        .on_press_tx(move |app: &mut App, tx| {
            tx.keep_on_screen(head_key.clone());
            toggle(app, &toggle_id)
        })
        .child(icon(glyph, 16.0).color(color))
        .child(text(title).font(theme::TEXT_SM).color(t.c.muted_foreground).truncate().grow(1.0).min_w(0.0))
        .child(icon(if expanded { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 12.0).color(t.c.muted_foreground));
    let mut out = col().w_full().gap(4.0).child(head);
    if expanded {
        let mut list = col().w_full().gap(4.0).pl(12.0);
        for item in items {
            list = list.child(item_view(app, item, t));
        }
        out = out.child(list);
    }
    out
}

/// The running turn's "Thinking" row while nothing else shows activity; `elapsed` is since its
/// last step, and only changes once a second.
pub fn thinking_view(elapsed: &str, t: &T) -> El {
    row()
        .w_full()
        .gap(8.0)
        .py(4.0)
        .px(4.0)
        .center_y()
        .label(format!("Thinking, {elapsed}"))
        .child(icon(&icons::BRAIN, 16.0).color(t.c.muted_foreground))
        .child(text("Thinking").font(theme::TEXT_SM).color(t.c.muted_foreground))
        .child(text(elapsed.to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground.alpha_mul(0.7)))
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
        assert_eq!(live_label(&json!({"type":"command_execution","status":"running","input":"vp test run\n--watch"})), "Running vp test run");
        assert_eq!(live_label(&json!({"type":"command_execution","status":"completed","input":"ls"})), "Ran ls");
        assert_eq!(live_label(&json!({"type":"reasoning","status":"running","text":""})), "Thinking");
        assert_eq!(live_label(&json!({"type":"file_search","title":"Searched src"})), "Searched src");
    }

    #[test]
    fn steers_say_how_they_reached_the_turn() {
        assert_eq!(steer(&json!({"inputIntent":"steer"})), Some("Steered the active turn"));
        assert!(steer(&json!({"inputIntent":"promoted_queued_to_steer"})).is_some_and(|h| h.starts_with("Originally queued")));
        assert_eq!(steer(&json!({"inputIntent":"turn_start"})), None);
    }
}
