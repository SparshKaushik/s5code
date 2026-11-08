//! The changed-files card under a settled turn's last reply (ChangedFilesCard, turnDiffTree.ts):
//! totals, Open diff, and the files as a tree whose folders start folded.

use std::collections::HashMap;

use rune::{El, col, icon, row, text};
use serde_json::Value;

use super::rows::array;
use crate::data::s;
use crate::kit::Button;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};

/// Each run's checkpoint that changed files, keyed by the run's last reply.
pub fn by_reply(p: &Value) -> HashMap<&str, &Value> {
    let mut last_reply = HashMap::new();
    for item in array(p, "turnItems") {
        if s(item, "type") == "assistant_message" && !s(item, "runId").is_empty() {
            last_reply.insert(s(item, "runId"), s(item, "id"));
        }
    }
    array(p, "checkpoints")
        .iter()
        .filter(|c| !c["appRunOrdinal"].is_null() && !array(c, "files").is_empty())
        .filter_map(|c| Some((*last_reply.get(s(c, "runId"))?, c)))
        .collect()
}

#[derive(Debug, PartialEq)]
pub struct Entry {
    pub depth: usize,
    pub name: String,
    /// The folder's path; None for a file.
    pub dir: Option<String>,
    pub additions: u64,
    pub deletions: u64,
}

#[derive(Default)]
struct Dir {
    dirs: Vec<(String, Dir)>,
    files: Vec<(String, u64, u64)>,
    stat: (u64, u64),
}

/// Folders first then files, each by name; a chain of lone folders reads as one
/// (`src/chat`). Only folders in `open` show their contents.
pub fn tree(files: &[Value], open: &dyn Fn(&str) -> bool) -> Vec<Entry> {
    let mut root = Dir::default();
    for f in files {
        let path = s(f, "path").replace('\\', "/");
        let parts: Vec<&str> = path.split('/').filter(|p| !p.is_empty()).collect();
        let Some((name, folders)) = parts.split_last() else { continue };
        let stat = (f["additions"].as_u64().unwrap_or(0), f["deletions"].as_u64().unwrap_or(0));
        let mut dir = &mut root;
        dir.stat = (dir.stat.0 + stat.0, dir.stat.1 + stat.1);
        for part in folders {
            let at = match dir.dirs.iter().position(|(n, _)| n == part) {
                Some(at) => at,
                None => {
                    dir.dirs.push((part.to_string(), Dir::default()));
                    dir.dirs.len() - 1
                }
            };
            dir = &mut dir.dirs[at].1;
            dir.stat = (dir.stat.0 + stat.0, dir.stat.1 + stat.1);
        }
        dir.files.push((name.to_string(), stat.0, stat.1));
    }
    fn walk(dir: &Dir, prefix: &str, depth: usize, open: &dyn Fn(&str) -> bool, out: &mut Vec<Entry>) {
        let mut dirs: Vec<&(String, Dir)> = dir.dirs.iter().collect();
        dirs.sort_by_key(|(n, _)| n.to_lowercase());
        for (name, sub) in dirs {
            let (mut label, mut sub) = (name.clone(), sub);
            while sub.files.is_empty() && sub.dirs.len() == 1 {
                label = format!("{label}/{}", sub.dirs[0].0);
                sub = &sub.dirs[0].1;
            }
            let path = if prefix.is_empty() { label.clone() } else { format!("{prefix}/{label}") };
            let expanded = open(&path);
            out.push(Entry { depth, name: label, dir: Some(path.clone()), additions: sub.stat.0, deletions: sub.stat.1 });
            if expanded {
                walk(sub, &path, depth + 1, open, out);
            }
        }
        let mut files: Vec<&(String, u64, u64)> = dir.files.iter().collect();
        files.sort_by_key(|(n, ..)| n.to_lowercase());
        for (name, add, del) in files {
            out.push(Entry { depth, name: name.clone(), dir: None, additions: *add, deletions: *del });
        }
    }
    let mut out = Vec::new();
    walk(&root, "", 0, open, &mut out);
    out
}

fn stat(add: u64, del: u64, font: rune::tok::TextStyle, t: &T) -> El {
    row()
        .gap(4.0)
        .child(text(format!("+{add}")).font(theme::mono(font)).color(t.c.diff_addition_foreground))
        .child(text(format!("-{del}")).font(theme::mono(font)).color(t.c.diff_deletion_foreground))
}

pub fn card(app: &App, checkpoint: &Value, t: &T) -> El {
    let files = array(checkpoint, "files");
    let totals = &checkpoint["filesTotal"];
    let sum = |key: &str| files.iter().filter_map(|f| f[key].as_u64()).sum::<u64>();
    let count = totals["count"].as_u64().unwrap_or(files.len() as u64);
    let (add, del) = (totals["additions"].as_u64().unwrap_or_else(|| sum("additions")), totals["deletions"].as_u64().unwrap_or_else(|| sum("deletions")));
    let run = s(checkpoint, "runId").to_owned();
    let fold_key = |path: &str| format!("changed:{run}:{path}");
    let expanded = &app.ui.chat.timeline.expanded;
    let entries = tree(files, &|path| expanded.contains(&fold_key(path)));
    let open_diff = {
        let checkpoint = checkpoint.clone();
        move |app: &mut App| crate::panel::open_diff(app, checkpoint.clone())
    };
    let mut head = row()
        .w_full()
        .px(12.0)
        .py(8.0)
        .gap(12.0)
        .center_y()
        .child(text(format!("{count} changed file{}", if count == 1 { "" } else { "s" })).font(theme::XS_MEDIUM).color(t.c.foreground));
    if add + del > 0 {
        head = head.child(stat(add, del, theme::TEXT_XS, t));
    }
    let head = head.child(rune::spacer()).child(
        Button::new("Open diff").variant("ghost-muted").size("xs").icon(&icons::FILE_DIFF).on_press(open_diff.clone()).build(t).help("Open the full diff"),
    );
    let nested = entries.iter().any(|e| e.dir.is_some());
    let mut list = col().w_full().px(4.0).pb(4.0);
    for entry in entries {
        let pad = 8.0 + entry.depth as f32 * 14.0;
        let mut line = row().w_full().h(28.0).pl(pad).pr(8.0).gap(8.0).center_y().radius(6.0).hover(t.c.accent.alpha_mul(0.6)).focusable(true).role("button").cursor(rune::Cursor::Pointer);
        line = match &entry.dir {
            Some(path) => {
                let key = fold_key(path);
                let open = app.ui.chat.timeline.expanded.contains(&key);
                line.label(entry.name.clone())
                    .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 14.0).color(t.c.muted_foreground.alpha_mul(0.7)))
                    .child(icon(if open { &icons::FOLDER_OPEN } else { &icons::FOLDER }, 14.0).color(t.c.muted_foreground.alpha_mul(0.75)))
                    .child(text(entry.name).font(theme::mono(theme::TEXT_2XS)).color(t.c.muted_foreground.alpha_mul(0.9)).truncate().shrink().min_w(0.0))
                    .on_press(move |app: &mut App| super::toggle(app, &key))
            }
            None => {
                let line = if nested || entry.depth > 0 { line.child(rune::el().size(14.0, 14.0)) } else { line };
                line.label(entry.name.clone())
                    .child(icon(&icons::FILE, 14.0).color(t.c.muted_foreground.alpha_mul(0.7)))
                    .child(text(entry.name).font(theme::mono(theme::TEXT_XS)).color(t.c.foreground.alpha_mul(0.85)).truncate().shrink().min_w(0.0))
                    .on_press(open_diff.clone())
            }
        };
        if entry.additions + entry.deletions > 0 {
            line = line.child(rune::spacer()).child(stat(entry.additions, entry.deletions, theme::TEXT_2XS, t));
        }
        list = list.child(line);
    }
    let hidden = count.saturating_sub(files.len() as u64);
    if hidden > 0 {
        list = list.child(
            row().px(8.0).child(
                Button::new(format!("{hidden} more file{} in the full diff", if hidden == 1 { "" } else { "s" }))
                    .variant("ghost-muted")
                    .size("xs")
                    .on_press(open_diff)
                    .build(t),
            ),
        );
    }
    col()
        .w_full()
        .mt(16.0)
        .radius(8.0)
        .fill(if t.dark { t.c.input.alpha_mul(0.2) } else { t.c.secondary })
        .child(head)
        .child(list)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn folders_come_first_fold_by_default_and_lone_chains_join() {
        let files = json!([
            {"path": "src/chat/a.rs", "additions": 2, "deletions": 1},
            {"path": "src/chat/b.rs", "additions": 1, "deletions": 0},
            {"path": "README.md", "additions": 4, "deletions": 0},
        ]);
        let files = files.as_array().unwrap();
        let folded = tree(files, &|_| false);
        let names: Vec<_> = folded.iter().map(|e| (e.depth, e.name.as_str(), e.additions)).collect();
        assert_eq!(names, [(0, "src/chat", 3), (0, "README.md", 4)]);
        let open = tree(files, &|p| p == "src/chat");
        let names: Vec<_> = open.iter().map(|e| (e.depth, e.name.as_str())).collect();
        assert_eq!(names, [(0, "src/chat"), (1, "a.rs"), (1, "b.rs"), (0, "README.md")]);
    }

    #[test]
    fn the_card_hangs_under_each_runs_last_reply() {
        let p = json!({
            "turnItems": [
                {"id": "a1", "type": "assistant_message", "runId": "r"},
                {"id": "a2", "type": "assistant_message", "runId": "r"},
                {"id": "b1", "type": "assistant_message", "runId": "q"},
            ],
            "checkpoints": [
                {"id": "c1", "runId": "r", "appRunOrdinal": 1, "files": [{"path": "x"}]},
                {"id": "c2", "runId": "q", "appRunOrdinal": 2, "files": []},
            ],
        });
        let map = by_reply(&p);
        assert_eq!(map.keys().collect::<Vec<_>>(), [&"a2"]);
    }
}
