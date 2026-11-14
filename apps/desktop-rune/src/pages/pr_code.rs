//! The pull request detail's Code tab (PullRequestCodeTab): the toolbar over the diff, files
//! folded and ticked viewed, the file tree, and line comments pending the review. The rows are
//! the Diff panel's (`panel::diff::view`), drawn over this tab's own `DiffState`.

use std::borrow::Cow;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use rune::{Color, Cursor, El, GrowthSpec, Key, KeyEvent, ScrollSpec, TextEdit, Tx, col, el, icon, memo, row, spacer, text, text_input};
use rune_kit::diff::{FileDiff, FileStatus, Hunk, Line, LineKind, Side};
use serde_json::{Value, json};

use super::avatars::avatar;
use super::pr_conversation::{list, scoped_commit};
use super::pr_detail::{file_counts, reference};
use crate::App;
use crate::data::{s as field, ts};
use crate::env::TAG;
use crate::icons as i;
use crate::kit::{self, Menu, MenuItem, Placement};
use crate::panel::diff::comments::{ReviewLine, find_index, range_label, record, review_lines};
use crate::panel::diff::state::{DiffMode, DiffState, HEADER_H, LINE_H, LineRange, LineRef, Note, Row};
use crate::panel::diff::view::{self, Host, Look};
use crate::panel::diff::{all_dirs, is_test_file, layout_toggle, save_layout, toggle, tree_rows};
use crate::theme::{self, AlphaMul, T, radius};

/// filesViewedStore's `flushDelayMs`.
const FLUSH_MS: i64 = 400;
/// The web keeps this in localStorage (`t3code.pullRequestFileTreeOpen`).
const TREE_KEY: &str = "pullRequestFileTreeOpen";
/// The web keeps this in localStorage (`t3code.diffHideTestFiles`).
const HIDE_TESTS_KEY: &str = "diffHideTestFiles";
const DRAFT: &str = "draft";
/// Prefixes a review thread's note id.
const THREAD: &str = "thread:";
/// The narrowest toolbar that keeps its view toggles out of the Diff options menu (`@lg`).
const WIDE_TOOLBAR: f32 = 512.0;
/// Past this many removed-by-added line pairs a change keeps its whitespace-only lines.
const WS_CELLS: usize = 1 << 20;
const REPLY_FAILED: &str = "Reply could not be posted";
const ENVIRONMENT_TICKS: &str = "This host keeps no shared record of which files you have read, so these ticks are kept by this environment. They follow you between the apps connected to it, but the host's own web UI will not show them.";

#[derive(Default)]
pub struct Ui {
    pub diff: DiffState,
    /// The detail read a diff that `diff` has yet to show.
    pub fresh: bool,
    /// The pull request and commit the choices below belong to (web's scopeKey).
    scope: String,
    /// Files whose fold differs from the default (toggledFiles).
    toggled: HashSet<String>,
    /// Expand all or Collapse all over the `diffFilesCollapsed` setting (foldOverride).
    fold: Option<bool>,
    /// The file whose Viewed box was pressed last, where a shift-press extends from.
    last_viewed: Option<String>,
    draft: Option<Draft>,
    edit: TextEdit,
    /// The wrap toggle, seeded from `wordWrap` and kept while the tab stays mounted.
    wrap: Option<bool>,
    folded_dirs: HashSet<String>,
    /// Line comments waiting for the review, by pullRequestReviewKey (pullRequestReviewStore).
    pending: HashMap<String, Vec<Value>>,
    /// A submitted review's key and the pending comment ids it carries.
    sent: Option<(String, Vec<String>)>,
    viewed: Viewed,
    /// The cursor of the diff's next page while there is one (nextCursor).
    pub next: Option<String>,
    /// The page read in flight and the scope it was asked for.
    paging: Option<(u64, String)>,
    page_failed: bool,
    /// Hide whitespace changes, seeded from `diffIgnoreWhitespace`.
    ignore_ws: Option<bool>,
    /// Each file's counts before whitespace is hidden (sourceFileStats).
    counts: HashMap<String, (usize, usize)>,
    orphans_open: bool,
    /// Review threads opened or folded against their default (open unless resolved).
    flipped: HashSet<String>,
    /// The thread being replied to, and the reply.
    replying: Option<(String, TextEdit)>,
    /// A reply or resolve in flight, and the toast its failure raises.
    thread_pending: Option<(u64, &'static str)>,
    /// The narrow toolbar's Diff options menu.
    options: bool,
    /// The file last revealed from outside the tab, shown even while tests are hidden (revealRequest).
    revealed: Option<String>,
}

/// The line comment being written (DraftAnchor).
struct Draft {
    path: String,
    old_path: Option<String>,
    position: Value,
    start: Option<Value>,
    /// The lines picked, in press order (range).
    range: (LineRef, LineRef),
}

/// One pull request's viewed ticks (filesViewedStore's scope): the host's answer, with presses
/// standing over it until a read could have seen them.
#[derive(Default)]
struct Viewed {
    /// filesViewedScopeKey.
    key: String,
    entry: Option<Value>,
    answer: Option<Value>,
    error: Option<String>,
    overlay: HashMap<String, bool>,
    /// Presses waiting out the debounce.
    queued: HashMap<String, bool>,
    flush_at: Option<i64>,
    /// The write in flight and the presses it carries.
    sending: Option<(u64, Vec<(String, bool)>)>,
    /// Paths whose write landed, with how many reads had begun by then.
    landed: HashMap<String, u64>,
    reads: u64,
    /// The read in flight and its place among the reads.
    reading: Option<(u64, u64)>,
    /// The answer predates a write or a newly read diff.
    refresh: bool,
    /// Presses sent while leaving a pull request, read back only to report a failure.
    leaving: Vec<(Value, u64)>,
}

impl Viewed {
    fn state(&self, path: &str) -> &str {
        let files = self.answer.as_ref().and_then(|a| a["files"].as_array());
        files.and_then(|f| f.iter().find(|f| field(f, "path") == path)).map_or("unviewed", |f| field(f, "state"))
    }

    /// isFileViewed.
    fn viewed(&self, path: &str) -> bool {
        self.overlay.get(path).copied().unwrap_or_else(|| self.state(path) == "viewed")
    }

    /// The file was pushed to since it was ticked (`dismissed`).
    fn changed(&self, path: &str) -> bool {
        !self.overlay.contains_key(path) && self.state(path) == "dismissed"
    }

    fn press(&mut self, path: &str, viewed: bool, now: i64) {
        self.queued.insert(path.to_owned(), viewed);
        self.overlay.insert(path.to_owned(), viewed);
        self.flush_at = Some(now + FLUSH_MS);
    }

    /// The queued presses once the debounce has passed and no write is in flight.
    fn take_batch(&mut self, now: i64) -> Option<Vec<(String, bool)>> {
        if self.sending.is_some() || self.queued.is_empty() || self.flush_at.is_none_or(|at| at > now) {
            return None;
        }
        self.flush_at = None;
        let mut batch: Vec<(String, bool)> = self.queued.drain().collect();
        batch.sort();
        Some(batch)
    }

    /// settleFileViewedOverlay with the answer of read `read`: a press retires once a read begun
    /// after its write landed answers, or the host agrees with it.
    fn settle(&mut self, answer: Value, read: u64) {
        self.answer = Some(answer);
        let sent: HashSet<&str> = self.sending.iter().flat_map(|(_, b)| b.iter().map(|(p, _)| p.as_str())).collect();
        let retired: Vec<String> = self
            .overlay
            .iter()
            .filter(|(p, pressed)| {
                !self.queued.contains_key(*p) && !sent.contains(p.as_str()) && (self.landed.get(*p).is_some_and(|&at| read >= at) || (self.state(p) == "viewed") == **pressed)
            })
            .map(|(p, _)| p.clone())
            .collect();
        for p in retired {
            self.overlay.remove(&p);
            self.landed.remove(&p);
        }
    }

    /// revertFileViewedOverlay for a failed write: returns whether any press was still its own.
    fn failed(&mut self, batch: &[(String, bool)]) -> bool {
        let mut owned = false;
        for (p, v) in batch.iter().filter(|(p, _)| !self.queued.contains_key(p)) {
            owned = true;
            if self.overlay.get(p) == Some(v) {
                self.overlay.remove(p);
            }
        }
        owned
    }
}

// ---- logic ------------------------------------------------------------------------------

fn setting(s: &App, key: &str, default: bool) -> bool {
    s.client_settings.values.get(key).and_then(Value::as_bool).unwrap_or(default)
}

fn layout(s: &App) -> DiffMode {
    if s.client_settings.values.get("diffLayout").and_then(Value::as_str) == Some("split") { DiffMode::Split } else { DiffMode::Unified }
}

fn wrap_on(s: &App) -> bool {
    s.ui.pages.code.wrap.unwrap_or_else(|| setting(s, "wordWrap", true))
}

fn tree_open(s: &App) -> bool {
    setting(s, TREE_KEY, false)
}

/// Whether files start folded: the toolbar's override, else `diffFilesCollapsed`.
fn fold_default(s: &App) -> bool {
    s.ui.pages.code.fold.unwrap_or_else(|| setting(s, "diffFilesCollapsed", true))
}

fn ignore_ws(s: &App) -> bool {
    s.ui.pages.code.ignore_ws.unwrap_or_else(|| setting(s, "diffIgnoreWhitespace", true))
}

fn hide_tests(s: &App) -> bool {
    setting(s, HIDE_TESTS_KEY, false)
}

/// The files shown: tests dropped while hidden, except one a reveal asked for, and whitespace-only
/// changes read as unchanged while hidden.
fn shown_files(source: &[FileDiff], hide: bool, ws: bool, kept: Option<&str>) -> Vec<FileDiff> {
    source.iter().filter(|f| !hide || !is_test_file(f.path()) || kept == Some(f.path())).map(|f| if ws { hide_whitespace(f) } else { f.clone() }).collect()
}

/// hideWhitespaceChanges: in each run of changed lines, a removed and an added line that differ
/// only in whitespace read as one unchanged line, as the new side has it.
fn hide_whitespace(file: &FileDiff) -> FileDiff {
    let mut out = file.clone();
    for h in &mut out.hunks {
        let lines = std::mem::take(&mut h.lines);
        let mut kept = Vec::with_capacity(lines.len());
        let mut at = 0;
        while at < lines.len() {
            if lines[at].kind == LineKind::Context {
                kept.push(lines[at].clone());
                at += 1;
                continue;
            }
            let end = (at..lines.len()).find(|&k| lines[k].kind == LineKind::Context).unwrap_or(lines.len());
            kept.extend(unify(&lines[at..end]));
            at = end;
        }
        h.lines = kept;
    }
    out
}

/// One run of changed lines with its whitespace-only pairs made unchanged, by the longest
/// common run of their lines stripped of whitespace.
fn unify(block: &[Line]) -> Vec<Line> {
    let old: Vec<&Line> = block.iter().filter(|l| l.kind == LineKind::Removed).collect();
    let new: Vec<&Line> = block.iter().filter(|l| l.kind == LineKind::Added).collect();
    let (n, m) = (old.len(), new.len());
    if n == 0 || m == 0 || n * m > WS_CELLS {
        return block.to_vec();
    }
    let bare = |l: &&Line| l.text.chars().filter(|c| !c.is_whitespace()).collect::<String>();
    let (a, b): (Vec<String>, Vec<String>) = (old.iter().map(bare).collect(), new.iter().map(bare).collect());
    let w = m + 1;
    let mut lcs = vec![0u32; (n + 1) * w];
    for i in (0..n).rev() {
        for j in (0..m).rev() {
            lcs[i * w + j] = if a[i] == b[j] { lcs[(i + 1) * w + j + 1] + 1 } else { lcs[(i + 1) * w + j].max(lcs[i * w + j + 1]) };
        }
    }
    if lcs[0] == 0 {
        return block.to_vec();
    }
    let (mut out, mut dels, mut adds) = (Vec::with_capacity(block.len()), vec![], vec![]);
    let (mut i, mut j) = (0, 0);
    while i < n && j < m {
        if a[i] == b[j] {
            out.append(&mut dels);
            out.append(&mut adds);
            let mut same = new[j].clone();
            same.kind = LineKind::Context;
            same.old = old[i].old;
            out.push(same);
            (i, j) = (i + 1, j + 1);
        } else if lcs[(i + 1) * w + j] >= lcs[i * w + j + 1] {
            dels.push(old[i].clone());
            i += 1;
        } else {
            adds.push(new[j].clone());
            j += 1;
        }
    }
    dels.extend(old[i..].iter().map(|l| (*l).clone()));
    adds.extend(new[j..].iter().map(|l| (*l).clone()));
    out.append(&mut dels);
    out.append(&mut adds);
    // Word changes paired lines that may no longer pair.
    for l in &mut out {
        l.changes.clear();
    }
    out
}

/// pullRequestReviewKey.
pub(super) fn review_key(entry: &Value) -> String {
    json!([entry["projectId"], entry["host"].as_str().map(str::to_lowercase), field(entry, "repository").to_lowercase(), entry["number"]]).to_string()
}

/// filesViewedScopeKey: the environment is part of it, since two can hand out the same project id.
fn viewed_key(entry: &Value) -> String {
    let r = reference(entry);
    json!([entry[TAG], r["projectId"], r["host"], r["expectedAccountId"], r["allowStale"], r["repository"], r["number"]]).to_string()
}

/// `PullRequestSetFilesViewedInput`.
fn set_viewed_input(entry: &Value, batch: &[(String, bool)]) -> Value {
    let mut input = reference(entry);
    input["files"] = Value::Array(batch.iter().map(|(path, viewed)| json!({"path": path, "viewed": viewed})).collect());
    input
}

/// isFileDiffCollapsed: a toggled file reads opposite to the default.
fn is_collapsed(path: &str, folded: bool, toggled: &HashSet<String>) -> bool {
    toggled.contains(path) != folded
}

/// toggleFileDiffFoldForViewed: folds `path` to `fold`, leaving it be when it already is.
fn fold_to(c: &mut Ui, default: bool, path: &str, fold: bool) {
    if is_collapsed(path, default, &c.toggled) != fold && !c.toggled.remove(path) {
        c.toggled.insert(path.to_owned());
    }
    if let Some(f) = c.diff.file_index(path) {
        c.diff.set_collapsed(f, fold);
    }
}

/// getFileViewedRange: the file, or every file from the anchor to it.
fn viewed_range<'a>(paths: &'a [String], path: &str, anchor: Option<&str>) -> &'a [String] {
    let Some(at) = paths.iter().position(|p| p == path) else { return &[] };
    match anchor.and_then(|a| paths.iter().position(|p| p == a)) {
        Some(from) => &paths[at.min(from)..=at.max(from)],
        None => &paths[at..=at],
    }
}

fn in_hunk(h: &Hunk, side: Side, line: u32) -> bool {
    match side {
        Side::Old => line >= h.old_start && line < h.old_start + h.old_count,
        Side::New => line >= h.new_start && line < h.new_start + h.new_count,
    }
}

/// reviewHunkIndex.
fn hunk_of(file: &FileDiff, p: &Value) -> Option<usize> {
    let (side, line) = if p["kind"] == "deleted" { (Side::Old, &p["oldLine"]) } else { (Side::New, &p["newLine"]) };
    let line = line.as_u64()? as u32;
    file.hunks.iter().position(|h| in_hunk(h, side, line))
}

/// resolveDiffReviewPosition: where the host anchors a comment on the line `at` names.
fn position(file: &FileDiff, lines: &[ReviewLine<'_>], at: LineRef) -> Option<Value> {
    let l = lines.get(find_index(lines, at)?)?;
    // An explicit side must not resolve to a same-numbered line on the other side.
    let exact = match at.side {
        Side::New => l.new == Some(at.line),
        Side::Old => l.old == Some(at.line),
    };
    if !exact {
        return None;
    }
    match l.kind {
        LineKind::Added => {
            let new = l.new?;
            Some(json!({"kind": "added", "newLine": new}))
        }
        LineKind::Removed => {
            let old = l.old?;
            Some(json!({"kind": "deleted", "oldLine": old}))
        }
        LineKind::Context => {
            let (old, new) = (l.old?, l.new?);
            let side = if at.side == Side::Old { "left" } else { "right" };
            file.hunks.iter().any(|h| in_hunk(h, Side::New, new)).then(|| json!({"kind": "context", "oldLine": old, "newLine": new, "side": side}))
        }
    }
}

/// resolveDiffReviewAnchor: the comment's `position`, and with ranged comments its first line
/// in diff order while the range stays in one hunk.
fn anchor(file: &FileDiff, lines: &[ReviewLine<'_>], start: LineRef, end: LineRef, ranged: bool) -> Option<(Value, Option<Value>)> {
    let last = position(file, lines, end)?;
    let Some(first) = ranged.then(|| position(file, lines, start)).flatten() else { return Some((last, None)) };
    let (a, b) = (find_index(lines, start), find_index(lines, end));
    if a == b {
        return Some((last, None));
    }
    let (from, to) = if a < b { (first, last.clone()) } else { (last.clone(), first) };
    let hunk = hunk_of(file, &to);
    Some(if hunk.is_some() && hunk_of(file, &from) == hunk { (to, Some(from)) } else { (last, None) })
}

/// getReviewPositionAnchor: the line a comment shows under.
fn anchor_line(p: &Value) -> Option<LineRef> {
    let n = |k: &str| p[k].as_u64().map(|n| n as u32);
    match p["kind"].as_str()? {
        "added" => Some(LineRef { side: Side::New, line: n("newLine")? }),
        "deleted" => Some(LineRef { side: Side::Old, line: n("oldLine")? }),
        _ if p["side"] == "left" => Some(LineRef { side: Side::Old, line: n("oldLine")? }),
        _ => Some(LineRef { side: Side::New, line: n("newLine")? }),
    }
}

/// A `PullRequestReviewCommentDraft` with the local id the pending list keys it by.
fn pending_comment(id: String, d: &Draft, body: &str) -> Value {
    let mut c = json!({"id": id, "path": d.path, "position": d.position, "body": body});
    if let Some(old) = &d.old_path {
        c["oldPath"] = json!(old);
    }
    if let Some(start) = &d.start {
        c["startPosition"] = start.clone();
    }
    c
}

fn covers(lines: &[ReviewLine<'_>], a: LineRef, b: LineRef, at: LineRef) -> bool {
    let (Some(a), Some(b), Some(at)) = (find_index(lines, a), find_index(lines, b), find_index(lines, at)) else { return false };
    a.min(b) <= at && at <= a.max(b)
}

/// canCommentOnLines: the host takes line comments from this viewer, on the whole change.
fn can_comment(s: &App) -> bool {
    let u = &s.ui.pages.detail;
    u.detail.as_ref().is_some_and(|d| d["capabilities"]["review"]["inlineComment"] == true && d["viewerPermissions"]["comment"] != false) && scoped_commit(u).is_none()
}

/// canSelectLines: lines go to the review where the host takes them, and to an agent always, but
/// only from the whole change.
fn can_select(s: &App) -> bool {
    let u = &s.ui.pages.detail;
    u.detail.is_some() && scoped_commit(u).is_none()
}

fn review_flag(s: &App, cap: &str, permission: &str) -> bool {
    s.ui.pages.detail.detail.as_ref().is_some_and(|d| d["capabilities"]["review"][cap] == true && d["viewerPermissions"][permission] == true)
}

fn writable(s: &App) -> bool {
    s.ui.pages.detail.entry.as_ref().is_some_and(|e| super::pr_row::can_write_on(s, e))
}

/// review.reply.
fn can_reply(s: &App) -> bool {
    review_flag(s, "reply", "comment")
}

/// review.resolve with canResolvePullRequestThread.
fn can_resolve(s: &App, th: &Value) -> bool {
    review_flag(s, "resolve", "resolve") && th["canResolve"] != false
}

/// The open pull request's review threads.
fn threads(s: &App) -> &[Value] {
    list(s.ui.pages.detail.activity.as_ref(), "reviewThreads")
}

/// isLineInFileDiff: where a review thread shows, when a hunk of its file as shown holds its line.
fn thread_anchor(st: &DiffState, th: &Value) -> Option<(usize, LineRef)> {
    let f = st.file_index(field(th, "path"))?;
    let line = th["line"].as_u64()? as u32;
    let side = if field(th, "side").eq_ignore_ascii_case("left") { Side::Old } else { Side::New };
    st.files()[f].hunks.iter().any(|h| in_hunk(h, side, line)).then_some((f, LineRef { side, line }))
}

/// The threads no shown line holds (orphanThreads): every one while a single commit is shown,
/// whose line numbers are another diff's.
fn orphan_threads<'a>(s: &'a App) -> Vec<&'a Value> {
    let whole = scoped_commit(&s.ui.pages.detail).is_none();
    threads(s).iter().filter(|th| !whole || thread_anchor(&s.ui.pages.code.diff, th).is_none()).collect()
}

/// The line comments pending `entry`'s review (usePendingReviewComments).
pub(super) fn pending<'a>(s: &'a App, entry: &Value) -> &'a [Value] {
    s.ui.pages.code.pending.get(&review_key(entry)).map(Vec::as_slice).unwrap_or(&[])
}

/// A review went out carrying `sent`; they leave the pending list once it lands.
pub(super) fn hold_submitted(s: &mut App, sent: &[Value]) {
    let Some(key) = s.ui.pages.detail.entry.as_ref().map(review_key) else { return };
    s.ui.pages.code.sent = Some((key, sent.iter().map(|p| field(p, "id").to_owned()).collect()));
}

/// The review landed: its comments go, any added while it was sending stay (removeComments).
pub(super) fn review_submitted(s: &mut App) {
    let Some((key, ids)) = s.ui.pages.code.sent.take() else { return };
    drop_pending(&mut s.ui.pages.code.pending, &key, |id| ids.iter().any(|sent| sent == id));
    sync_notes(s);
}

/// The composer's "Discard pending line comments".
pub(super) fn discard_all(s: &mut App) {
    let Some(key) = s.ui.pages.detail.entry.as_ref().map(review_key) else { return };
    s.ui.pages.code.pending.remove(&key);
    sync_notes(s);
}

/// A line comment written elsewhere (a guide excerpt) joins `entry`'s pending ones.
pub(super) fn add_pending(s: &mut App, entry: &Value, comment: Value) {
    s.ui.pages.code.pending.entry(review_key(entry)).or_default().push(comment);
    sync_notes(s);
}

pub(super) fn remove_pending(s: &mut App, entry: &Value, id: &str) {
    drop_pending(&mut s.ui.pages.code.pending, &review_key(entry), |p| p == id);
    sync_notes(s);
}

fn drop_pending(pending: &mut HashMap<String, Vec<Value>>, key: &str, gone: impl Fn(&str) -> bool) {
    if let Some(list) = pending.get_mut(key) {
        list.retain(|p| !gone(field(p, "id")));
        if list.is_empty() {
            pending.remove(key);
        }
    }
}

/// `capabilities.viewedFiles`: where the ticks are kept, when this host keeps them at all.
fn viewed_store(s: &App) -> Option<&str> {
    s.ui.pages.detail.detail.as_ref().and_then(|d| d["capabilities"]["viewedFiles"].as_str())
}

/// Keeps the tab's choices on the shown pull request and commit, puts a newly read diff in the
/// view, and reads and writes the viewed ticks.
pub(super) fn poll(s: &mut App) -> bool {
    let Some(entry) = s.ui.pages.detail.entry.clone() else { return false };
    let commit = scoped_commit(&s.ui.pages.detail);
    let scope = json!([entry[TAG], review_key(&entry), commit]).to_string();
    let (mode, wrap) = (layout(s), wrap_on(s));
    let mut changed = false;
    let c = &mut s.ui.pages.code;
    if c.scope != scope {
        // An open composer would otherwise attach its comment to whichever change is shown.
        c.scope = scope;
        c.toggled.clear();
        c.fold = None;
        c.last_viewed = None;
        c.draft = None;
        c.edit = TextEdit::default();
        c.folded_dirs.clear();
        c.diff.set_selection(None);
        c.orphans_open = false;
        c.revealed = None;
        c.replying = None;
        c.next = None;
        c.paging = None;
        c.page_failed = false;
        changed = true;
    }
    c.diff.set_mode(mode);
    c.diff.set_wrap(wrap);
    if std::mem::take(&mut c.fresh) {
        // A new first page: pages read for the old one target another diff.
        c.paging = None;
        c.page_failed = false;
        c.viewed.refresh = true;
        show(s);
        changed = true;
    }
    if s.ui.pages.detail.files.is_some() {
        for path in std::mem::take(&mut s.ui.pages.detail.expanded) {
            if s.ui.pages.code.diff.file_index(&path).is_none() {
                s.ui.pages.code.revealed = Some(path.clone());
                show(s);
            }
            unfold(s, &path);
            changed = true;
        }
        changed |= sync_page(s, &entry) | sync_notes(s);
    }
    changed |= sync_thread_command(s, &entry);
    sync_viewed(s, &entry) | changed
}

fn show(s: &mut App) {
    let (default, ws, hide) = (fold_default(s), ignore_ws(s), hide_tests(s));
    let u = &s.ui.pages.detail;
    let source = u.files.as_deref().unwrap_or(&[]);
    let files = shown_files(source, hide, ws, s.ui.pages.code.revealed.as_deref());
    let counts: HashMap<String, (usize, usize)> = source.iter().map(|f| (f.path().to_owned(), file_counts(f))).collect();
    let c = &mut s.ui.pages.code;
    c.counts = counts;
    let collapsed: Vec<String> = files.iter().map(FileDiff::path).filter(|p| is_collapsed(p, default, &c.toggled)).map(str::to_owned).collect();
    c.diff.set_files(files, &collapsed);
    sync_notes(s);
}

/// Takes the next page of the diff, adding its files below the ones shown.
fn sync_page(s: &mut App, entry: &Value) -> bool {
    let Some((id, asked)) = s.ui.pages.code.paging.clone() else { return false };
    let Some(got) = reply(s, entry, id) else { return false };
    s.ui.pages.code.paging = None;
    if asked != s.ui.pages.code.scope {
        return true;
    }
    match got {
        Ok(d) => {
            let more = rune_kit::diff::parse(d["patch"].as_str().unwrap_or(""));
            let u = &mut s.ui.pages.detail;
            if let Some(files) = &mut u.files {
                files.extend(more);
            }
            u.diff_truncated |= d["truncated"].as_bool().unwrap_or(false);
            s.ui.pages.code.next = d["nextCursor"].as_str().map(str::to_owned);
            show(s);
        }
        Err(_) => s.ui.pages.code.page_failed = true,
    }
    true
}

/// loadNextSlice: reads the page after the ones shown, once at a time.
fn load_more(s: &mut App) -> bool {
    let c = &s.ui.pages.code;
    if c.paging.is_some() || c.page_failed {
        return false;
    }
    let (Some(cursor), Some(entry)) = (c.next.clone(), s.ui.pages.detail.entry.clone()) else { return false };
    let mut input = reference(&entry);
    input["cursor"] = json!(cursor);
    if let Some(oid) = scoped_commit(&s.ui.pages.detail) {
        input["commit"] = json!(oid);
    }
    let scope = s.ui.pages.code.scope.clone();
    let Some(net) = super::pull_requests::net_for(s, &entry) else { return false };
    let id = net.http_request("POST", "/api/pull-requests/diff", input);
    s.ui.pages.code.paging = Some((id, scope));
    true
}

/// Retry, or the file tree's "Load more files".
fn retry_page(s: &mut App) {
    s.ui.pages.code.page_failed = false;
    load_more(s);
}

/// Puts the review threads, the pending comments and the open draft under their lines.
fn sync_notes(s: &mut App) -> bool {
    let u = &s.ui.pages.detail;
    let key = u.entry.as_ref().map(review_key);
    let whole = scoped_commit(u).is_none();
    let threads = list(u.activity.as_ref(), "reviewThreads");
    let c = &mut s.ui.pages.code;
    let index = |path: &str| c.diff.file_index(path);
    let mut notes = vec![];
    // Commit diffs use other line coordinates.
    if whole {
        for th in threads {
            if let Some((file, at)) = thread_anchor(&c.diff, th) {
                notes.push(Note { file, at, id: format!("{THREAD}{}", field(th, "id")), draft: false });
            }
        }
        for p in key.and_then(|k| c.pending.get(&k)).into_iter().flatten() {
            if let (Some(file), Some(at)) = (index(field(p, "path")), anchor_line(&p["position"])) {
                notes.push(Note { file, at, id: field(p, "id").to_owned(), draft: false });
            }
        }
    }
    if let Some(d) = &c.draft
        && let (Some(file), Some(at)) = (index(&d.path), anchor_line(&d.position))
    {
        notes.push(Note { file, at, id: DRAFT.into(), draft: true });
    }
    c.diff.set_notes(notes)
}

/// The viewed ticks' reads and debounced writes, on the open pull request's own environment.
fn sync_viewed(s: &mut App, entry: &Value) -> bool {
    let mut changed = false;
    let key = viewed_key(entry);
    if s.ui.pages.code.viewed.key != key {
        // Leaving a pull request sends its last presses to it.
        let old = std::mem::take(&mut s.ui.pages.code.viewed);
        let mut leaving = old.leaving;
        if let Some(prev) = old.entry.filter(|_| !old.queued.is_empty())
            && let Some(net) = super::pull_requests::net_for(s, &prev)
        {
            let mut batch: Vec<(String, bool)> = old.queued.into_iter().collect();
            batch.sort();
            let id = net.request("pullRequests.setFilesViewed", set_viewed_input(&prev, &batch));
            leaving.push((prev, id));
        }
        s.ui.pages.code.viewed = Viewed { key, entry: Some(entry.clone()), leaving, ..Viewed::default() };
    }
    let mut kept = vec![];
    for (prev, id) in std::mem::take(&mut s.ui.pages.code.viewed.leaving) {
        match reply(s, &prev, id) {
            None => kept.push((prev, id)),
            Some(Err(_)) => crate::shell::palette::toast_error(s, "Could not update viewed files", None),
            Some(Ok(_)) => {}
        }
    }
    s.ui.pages.code.viewed.leaving = kept;
    if viewed_store(s).is_none() || s.ui.pages.detail.files.is_none() {
        return changed;
    }
    if let Some((id, read)) = s.ui.pages.code.viewed.reading
        && let Some(got) = reply(s, entry, id)
    {
        let v = &mut s.ui.pages.code.viewed;
        v.reading = None;
        match got {
            Ok(answer) => {
                v.error = None;
                v.settle(answer, read);
            }
            Err(e) => v.error = Some(super::pull_requests::error_text(&e, "")),
        }
        changed = true;
    }
    if let Some(id) = s.ui.pages.code.viewed.sending.as_ref().map(|(id, _)| *id)
        && let Some(got) = reply(s, entry, id)
    {
        let v = &mut s.ui.pages.code.viewed;
        let (_, batch) = v.sending.take().unwrap_or_default();
        v.refresh = true;
        let failed = match got {
            Ok(_) => {
                let at = v.reads;
                for (p, _) in &batch {
                    v.landed.insert(p.clone(), at);
                }
                false
            }
            Err(_) => v.failed(&batch),
        };
        if failed {
            crate::shell::palette::toast_error(s, "Could not update viewed files", None);
        }
        changed = true;
    }
    if super::pull_requests::net_for(s, entry).is_none() {
        return changed;
    }
    let v = &mut s.ui.pages.code.viewed;
    let read = v.reading.is_none() && (v.refresh || (v.answer.is_none() && v.error.is_none()));
    let batch = v.take_batch(crate::data::now_ms());
    if let Some(net) = super::pull_requests::net_for(s, entry) {
        let read_id = read.then(|| net.request("pullRequests.filesViewed", reference(entry)));
        let write_id = batch.as_ref().map(|b| net.request("pullRequests.setFilesViewed", set_viewed_input(entry, b)));
        let v = &mut s.ui.pages.code.viewed;
        if let Some(id) = read_id {
            v.reading = Some((id, v.reads));
            v.reads += 1;
            v.refresh = false;
        }
        if let (Some(id), Some(b)) = (write_id, batch) {
            v.sending = Some((id, b));
        }
    }
    changed
}

/// The answer to `id`, sent to the environment that listed `entry`.
fn reply(s: &mut App, entry: &Value, id: u64) -> Option<Result<Value, Value>> {
    let env = entry[TAG].as_str().unwrap_or_default().to_owned();
    super::pull_requests::take(s, &env, id)
}

/// Whether a line comment is being written.
pub fn drafting(s: &App) -> bool {
    s.ui.pages.code.draft.is_some()
}

/// The hunks' lines of a file as the host sent it, before whitespace was hidden.
fn hunk_lines(file: &FileDiff) -> Vec<ReviewLine<'_>> {
    file.hunks.iter().flat_map(|h| &h.lines).map(|l| ReviewLine { kind: l.kind, old: l.old, new: l.new, text: &l.text }).collect()
}

/// beginComment: a draft on the selected lines when they hold `at`, else on `at` alone, anchored
/// where the host's own diff has them. A host without ranged comments takes the range's last line.
pub fn begin(s: &mut App, f: usize, at: LineRef) {
    if !can_select(s) {
        return;
    }
    let ranged = s.ui.pages.detail.detail.as_ref().is_some_and(|d| d["capabilities"]["review"]["rangeComment"] == true);
    let draft = {
        let st = &s.ui.pages.code.diff;
        let Some(file) = st.files().get(f) else { return };
        let lines = review_lines(st, f);
        let (start, end) = match st.selection() {
            Some(sel) if sel.file == f && covers(&lines, sel.start, sel.end, at) => (sel.start, sel.end),
            _ => (at, at),
        };
        let path = file.path().to_owned();
        let source = s.ui.pages.detail.files.as_deref().unwrap_or(&[]).iter().find(|src| src.path() == path);
        let hosted = source.and_then(|src| anchor(src, &hunk_lines(src), start, end, ranged));
        let Some((position, first)) = hosted.or_else(|| anchor(file, &lines, start, end, ranged)) else { return };
        Draft { old_path: file.old_path.clone().filter(|o| *o != path), path, position, start: first, range: (start, end) }
    };
    let c = &mut s.ui.pages.code;
    c.draft = Some(draft);
    c.diff.hushed = Some((f, at));
    c.edit = TextEdit::new("");
    c.edit.focused = true;
    sync_notes(s);
}

fn cancel(s: &mut App) {
    let c = &mut s.ui.pages.code;
    c.draft = None;
    c.edit = TextEdit::default();
    c.diff.set_selection(None);
    sync_notes(s);
}

/// "Add to review": the draft joins the comments the next review carries, and the composer
/// opens on the review it started.
fn add_to_review(s: &mut App) {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    if !can_comment(s) {
        return;
    }
    let Some(key) = s.ui.pages.detail.entry.as_ref().map(review_key) else { return };
    let c = &mut s.ui.pages.code;
    let body = c.edit.text.trim().to_owned();
    if body.is_empty() {
        return;
    }
    let Some(d) = c.draft.take() else { return };
    let id = format!("pending-review-comment-{}", SEQ.fetch_add(1, Ordering::Relaxed) + 1);
    c.pending.entry(key).or_default().push(pending_comment(id, &d, &body));
    c.edit = TextEdit::default();
    c.diff.set_selection(None);
    s.ui.pages.detail.edit.composer.review = true;
    sync_notes(s);
}

/// The selected lines as a review-comment context with an empty note (buildDiffReviewComment).
fn selection_context(st: &DiffState, d: &Draft, number: i64, body: &str) -> Option<Value> {
    let lines = review_lines(st, st.file_index(&d.path)?);
    let (a, b) = (find_index(&lines, d.range.0)?, find_index(&lines, d.range.1)?);
    let (section, title) = (format!("pull-request:{number}"), format!("PR #{number} review"));
    let id = format!("pull-request-selection:{}:{}:{}", d.path, d.range.0.line, d.range.1.line);
    let mut comment = record((section.as_str(), title.as_str()), &d.path, &lines, a, b, body, &id);
    comment["text"] = json!("");
    Some(comment)
}

/// "Add to agent" (addSelectionToAgent): a new thread holding the request, the pull request and
/// the lines.
fn add_to_agent(s: &mut App) {
    let body = s.ui.pages.code.edit.text.trim().to_owned();
    let Some(d) = s.ui.pages.detail.detail.clone() else { return };
    if body.is_empty() {
        return;
    }
    let Some(draft) = s.ui.pages.code.draft.take() else { return };
    let comment = selection_context(&s.ui.pages.code.diff, &draft, d["number"].as_i64().unwrap_or(0), &body);
    let c = &mut s.ui.pages.code;
    c.edit = TextEdit::default();
    c.diff.set_selection(None);
    sync_notes(s);
    let Some(comment) = comment else { return };
    let chip = crate::chat::composer::chips::pull_request(&d, true, &[]);
    super::pr_detail::handoff(s, super::pr_detail::bounded(&body), Some(chip));
    crate::chat::composer::add_context(s, comment);
}

/// The draft's primary button: the review where the host takes line comments, else the agent.
fn submit_draft(s: &mut App) {
    if can_comment(s) { add_to_review(s) } else { add_to_agent(s) }
}

fn discard(s: &mut App, id: &str) {
    let Some(key) = s.ui.pages.detail.entry.as_ref().map(review_key) else { return };
    drop_pending(&mut s.ui.pages.code.pending, &key, |p| p == id);
    sync_notes(s);
}

/// runThreadCommand: a reply or a resolve, one at a time; the pull request rereads once it lands.
fn thread_command(s: &mut App, method: &str, fields: Value, failure: &'static str) {
    if s.ui.pages.code.thread_pending.is_some() || !writable(s) {
        return;
    }
    let Some(entry) = s.ui.pages.detail.entry.clone() else { return };
    let mut input = reference(&entry);
    if let (Some(p), Value::Object(f)) = (input.as_object_mut(), fields) {
        p.extend(f);
    }
    match super::pull_requests::net_for(s, &entry) {
        Some(net) => {
            let id = net.request(method, input);
            s.ui.pages.code.thread_pending = Some((id, failure));
        }
        None => crate::shell::palette::toast_error(s, failure, None),
    }
}

fn sync_thread_command(s: &mut App, entry: &Value) -> bool {
    let Some((id, failure)) = s.ui.pages.code.thread_pending else { return false };
    let Some(got) = reply(s, entry, id) else { return false };
    s.ui.pages.code.thread_pending = None;
    match got {
        Ok(_) => {
            if failure == REPLY_FAILED {
                s.ui.pages.code.replying = None;
            }
            super::pr_detail::refresh(s);
        }
        Err(_) => crate::shell::palette::toast_error(s, failure, None),
    }
    true
}

fn send_reply(s: &mut App) {
    let Some((thread, edit)) = &s.ui.pages.code.replying else { return };
    let body = edit.text.trim().to_owned();
    if body.is_empty() {
        return;
    }
    let fields = json!({"threadId": thread, "body": body});
    thread_command(s, "pullRequests.replyToThread", fields, REPLY_FAILED);
}

fn resolve(s: &mut App, thread: &str, resolved: bool) {
    thread_command(s, "pullRequests.setThreadResolution", json!({"threadId": thread, "resolved": resolved}), "The conversation could not be updated");
}

fn start_reply(s: &mut App, thread: &str) {
    let mut edit = TextEdit::new("");
    edit.focused = true;
    s.ui.pages.code.replying = Some((thread.to_owned(), edit));
}

fn flip(s: &mut App, thread: &str) {
    let flipped = &mut s.ui.pages.code.flipped;
    if !flipped.remove(thread) {
        flipped.insert(thread.to_owned());
    }
}

fn reply_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    match e.key {
        Key::Escape => {
            s.ui.pages.code.replying = None;
            tx.blur();
            true
        }
        Key::Enter if e.mods.cmd || e.mods.ctrl => {
            send_reply(s);
            true
        }
        _ => false,
    }
}

fn editor_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    match e.key {
        Key::Escape => {
            cancel(s);
            tx.blur();
            true
        }
        Key::Enter if e.mods.cmd || e.mods.ctrl => {
            submit_draft(s);
            tx.blur();
            true
        }
        _ => false,
    }
}

/// setFileViewed: ticks `path`, or with shift every file from the last one pressed, folding the
/// ticked files and opening the unticked.
fn set_viewed(s: &mut App, path: &str, viewed: bool, extend: bool, tx: &mut Tx) {
    let default = fold_default(s);
    let now = crate::data::now_ms();
    let c = &mut s.ui.pages.code;
    let paths: Vec<String> = c.diff.files().iter().map(|f| f.path().to_owned()).collect();
    let range = viewed_range(&paths, path, if extend { c.last_viewed.as_deref() } else { None });
    c.last_viewed = Some(path.to_owned());
    for p in range {
        c.viewed.press(p, viewed, now);
        fold_to(c, default, p, viewed);
    }
    tx.wake_after(Duration::from_millis(FLUSH_MS as u64));
}

fn toggle_file(s: &mut App, path: &str, sticky: bool, tx: &mut Tx) {
    let default = fold_default(s);
    let c = &mut s.ui.pages.code;
    let Some(f) = c.diff.file_index(path) else { return };
    let fold = !c.diff.collapsed(f);
    fold_to(c, default, path, fold);
    if sticky {
        c.diff.scroll_to_file(f, tx);
    }
}

/// Opens `path` without ever folding it, as a reveal must; returns its index.
pub(super) fn unfold(s: &mut App, path: &str) -> Option<usize> {
    let default = fold_default(s);
    let c = &mut s.ui.pages.code;
    fold_to(c, default, path, false);
    c.diff.file_index(path)
}

/// revealFile from the file tree.
fn reveal(s: &mut App, path: &str, tx: &mut Tx) {
    if let Some(f) = unfold(s, path) {
        s.ui.pages.code.diff.scroll_to_file(f, tx);
    }
}

fn toggle_all(s: &mut App) {
    let c = &mut s.ui.pages.code;
    let fold = !c.diff.all_collapsed();
    c.fold = Some(fold);
    c.toggled.clear();
    c.diff.set_all_collapsed(fold);
}

fn pick_layout(s: &mut App, m: DiffMode) {
    s.ui.pages.code.diff.set_mode(m);
    save_layout(s, m);
}

fn set_wrap(s: &mut App, wrap: bool) {
    let c = &mut s.ui.pages.code;
    c.wrap = Some(wrap);
    c.diff.set_wrap(wrap);
}

/// changeIgnoreWhitespace: the draft's lines may not survive the change.
fn set_ignore_ws(s: &mut App, on: bool) {
    let c = &mut s.ui.pages.code;
    c.ignore_ws = Some(on);
    c.draft = None;
    c.edit = TextEdit::default();
    show(s);
}

fn set_hide_tests(s: &mut App, hide: bool) {
    s.client_settings.values.insert(HIDE_TESTS_KEY.into(), json!(hide));
    s.client_settings.save();
    show(s);
}

fn set_tree(s: &mut App, open: bool) {
    s.client_settings.values.insert(TREE_KEY.into(), json!(open));
    s.client_settings.save();
}

// ---- view -------------------------------------------------------------------------------

/// A toolbar mark whose tooltip explains it.
fn hint(glyph: &'static rune::Icon, ink: Color, label: &str, help: String) -> El {
    row().center().label(label.to_owned()).help(help).child(icon(glyph, 14.0).color(ink))
}

/// The toolbar: the diff scope menu and what the files hold, then the view toggles.
pub(super) fn toolbar(s: &App, t: &T, overlays: &mut Vec<El>) -> El {
    let u = &s.ui.pages.detail;
    let c = &s.ui.pages.code;
    let (muted, warn) = (t.c.muted_foreground, t.c.warning_foreground);
    let tnum = theme::TEXT_XS.features("'tnum' 1");
    let mut left = row().grow(1.0).shrink().min_w(0.0).gap(12.0).center_y();
    if let Some(menu) = super::pr_conversation::scope_menu(s, t, overlays) {
        left = left.child(menu);
    }
    let source = u.files.as_deref().unwrap_or(&[]);
    let files = c.diff.files();
    let n = files.len();
    let more = if c.next.is_some() { "+" } else { "" };
    let mut meta = vec![text(format!("{n} {}{more}", if n == 1 { "file" } else { "files" })).font(tnum).color(muted)];
    let hidden = source.len().saturating_sub(n);
    if hidden > 0 && hide_tests(s) {
        let label = format!("{hidden} test {} hidden", if hidden == 1 { "file" } else { "files" });
        meta.push(kit::Button::new(label).variant("link").size("xs").on_press(|s: &mut App| set_hide_tests(s, false)).build(t));
    }
    if let Some(store) = viewed_store(s).filter(|_| n > 0) {
        let v = &c.viewed;
        let ticked = files.iter().filter(|f| v.viewed(f.path())).count();
        let here = store == "environment";
        let mut seg = row()
            .gap(4.0)
            .center_y()
            .min_w(0.0)
            .shrink()
            .child(text(format!("{ticked} / {n}")).font(tnum).color(muted))
            .child(text(if here { "viewed in S5 Code" } else { "viewed" }).font(theme::TEXT_XS).color(muted).truncate().min_w(0.0).shrink());
        if here {
            seg = seg.child(hint(&i::INFO, muted, "These ticks are kept here, not on the host", ENVIRONMENT_TICKS.into()));
        }
        if let Some(e) = &v.error {
            seg = seg.child(hint(&i::TRIANGLE_ALERT, warn, "Your ticks could not be read", format!("The boxes below are whatever was last read, and empty if nothing has been read yet. {e}")));
        }
        if v.answer.as_ref().is_some_and(|a| a["truncated"] == true) {
            let why = "This change has more files than the host will report ticks for in one read, so the count is short and some boxes below start empty.";
            seg = seg.child(hint(&i::TRIANGLE_ALERT, warn, "This count covers only part of the change", why.into()));
        }
        meta.push(seg);
    }
    if u.diff_truncated {
        meta.push(hint(&i::TRIANGLE_ALERT, warn, "Some of this diff was not shown", "The host withheld part of this diff: a binary file, or a change too large to inline.".into()));
    }
    let inline = u.detail.as_ref().is_some_and(|d| d["capabilities"]["review"]["inlineComment"] == true && d["viewerPermissions"]["comment"] != false);
    if inline && scoped_commit(u).is_some() {
        let why = "A comment is anchored to the whole change, so switch to All commits to write one.";
        meta.push(hint(&i::MESSAGE_SQUARE_OFF, muted, "Line comments are written from the whole change", why.into()));
    }
    let mut line = row().gap(6.0).center_y().min_w(0.0).shrink();
    for (k, seg) in meta.into_iter().enumerate() {
        if k > 0 {
            line = line.child(text("·").font(theme::TEXT_XS).color(muted.alpha_mul(0.5)));
        }
        line = line.child(seg);
    }
    left = left.child(line);
    let (wrap, ignore, mode) = (wrap_on(s), ignore_ws(s), c.diff.mode());
    let mut right = row().gap(4.0).center_y();
    // The toolbar's own width, as web's `@lg` container query reads it.
    if rune::memo::layout_with("pr-code-toolbar", |r| r[2] < WIDE_TOOLBAR).unwrap_or(false) {
        let trigger = kit::Button::new("Diff options")
            .icon(&i::SETTINGS_2)
            .variant("ghost")
            .size("icon-sm")
            .on_press(|s: &mut App| {
                let c = &mut s.ui.pages.code;
                c.options = !c.options;
            })
            .build(t)
            .help("Diff options");
        let pick = |m: DiffMode| {
            move |s: &mut App| {
                pick_layout(s, m);
                s.ui.pages.code.options = false;
            }
        };
        let menu = Menu::new("pr-code-options")
            .trigger(trigger)
            .placement(Placement::BottomEnd)
            .open(c.options)
            .on_close(|s: &mut App| s.ui.pages.code.options = false)
            .item(MenuItem::new("Hide whitespace changes").checked(ignore).on_select(move |s: &mut App| set_ignore_ws(s, !ignore)))
            .item(MenuItem::new("Wrap lines").checked(wrap).on_select(move |s: &mut App| set_wrap(s, !wrap)))
            .separator()
            .item(MenuItem::new("Stacked").radio(mode == DiffMode::Unified).on_select(pick(DiffMode::Unified)))
            .item(MenuItem::new("Split").radio(mode == DiffMode::Split).on_select(pick(DiffMode::Split)));
        right = right.child(menu.build(&kit::theme(t), overlays));
    } else {
        right = right
            .child(toggle(t, "pr-code-whitespace", if ignore { "Show whitespace changes" } else { "Hide whitespace changes" }, &i::PILCROW, ignore, move |s: &mut App| set_ignore_ws(s, !ignore)))
            .child(layout_toggle(t, "pr-code", ("Stacked diff view", "Split diff view"), mode, pick_layout))
            .child(
                toggle(t, "pr-code-wrap", if wrap { "Disable line wrapping" } else { "Enable line wrapping" }, &i::TEXT_WRAP, wrap, move |s: &mut App| set_wrap(s, !wrap))
                    .label(if wrap { "Disable diff line wrapping" } else { "Enable diff line wrapping" }),
            );
    }
    if n > 0 {
        let all = c.diff.all_collapsed();
        let label = if all { "Expand all files" } else { "Collapse all files" };
        let glyph = if all { &i::CHEVRONS_UP_DOWN } else { &i::CHEVRONS_DOWN_UP };
        right = right.child(kit::Button::new(label).icon(glyph).variant("ghost").size("icon-sm").on_press(toggle_all).build(t).key("pr-code-collapse").help(label));
    }
    if source.iter().any(|f| is_test_file(f.path())) {
        let hide = hide_tests(s);
        right = right.child(toggle(t, "pr-code-tests", if hide { "Show test files" } else { "Hide test files" }, &i::FLASK_CONICAL_OFF, hide, move |s: &mut App| set_hide_tests(s, !hide)).label("Hide test files"));
    }
    if n > 0 {
        let open = tree_open(s);
        right = right.child(toggle(t, "pr-code-tree", if open { "Hide file tree" } else { "Show file tree" }, &i::FOLDER_TREE, open, move |s: &mut App| set_tree(s, !open)));
    }
    row().key("pr-code-toolbar").w_full().h(40.0).px(16.0).gap(8.0).center_y().child(left).child(right)
}

/// The loaded diff, beside the file tree when it is open, under the conversations it has no line for.
pub(super) fn view(s: &App, t: &T) -> El {
    let mut main = row().grow(1.0).min_h(0.0).w_full().items_stretch().child(diff_view(s, t));
    if tree_open(s) && !s.ui.pages.code.diff.files().is_empty() {
        main = main.child(el().w(1.0).self_stretch().fill(t.c.border.alpha_mul(0.6))).child(tree(s, t));
    }
    col().grow(1.0).min_h(0.0).w_full().children(orphans(s, t)).child(main)
}

/// The review threads no shown line holds, by file, folded under their count.
fn orphans(s: &App, t: &T) -> Option<El> {
    let lost = orphan_threads(s);
    if lost.is_empty() {
        return None;
    }
    let c = &s.ui.pages.code;
    let muted = t.c.muted_foreground;
    let n = lost.len();
    let label = if c.next.is_none() { "Conversations not on the current diff" } else { "Conversations not on the diff loaded so far" };
    let open = c.orphans_open;
    let count = if n == 1 { "1 conversation".to_string() } else { format!("{n} conversations") };
    let head = row()
        .key("pr-code-orphans-toggle")
        .w_full()
        .px(16.0)
        .py(8.0)
        .gap(6.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .role("button")
        .expanded(open)
        .label(format!("{label}, {count}"))
        .child(text(label).font(theme::TEXT_XS).color(muted))
        .child(icon(if open { &i::CHEVRON_DOWN } else { &i::CHEVRON_RIGHT }, 14.0).color(muted))
        .child(text(n.to_string()).font(theme::TEXT_XS.features("'tnum' 1")).color(muted))
        .on_press(|s: &mut App| {
            let c = &mut s.ui.pages.code;
            c.orphans_open = !c.orphans_open;
        });
    let mut out = col().key("pr-code-orphans").w_full().child(head);
    if open {
        let mut groups: Vec<(&str, Vec<&Value>)> = vec![];
        for th in lost {
            let path = field(th, "path");
            match groups.iter_mut().find(|g| g.0 == path) {
                Some(g) => g.1.push(th),
                None => groups.push((path, vec![th])),
            }
        }
        let mut body = col().w_full().px(16.0).pb(12.0).gap(12.0);
        for (path, group) in groups {
            let mut files = col().w_full().gap(4.0).child(text(path.to_owned()).font(theme::TEXT_XS).color(muted).truncate().min_w(0.0).px(12.0).help(path.to_owned()));
            for th in group {
                let mut one = col().w_full().gap(4.0);
                if let Some(line) = th["line"].as_u64() {
                    one = one.child(text(format!("Line {line}")).font(theme::TEXT_XS).color(muted).px(12.0));
                }
                files = files.child(one.child(thread_card(s, t, th)));
            }
            body = body.child(files);
        }
        out = out.child(col().w_full().max_h(256.0).min_h(0.0).child(body).scroll(ScrollSpec { stick_bottom: false }));
    }
    Some(out.child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.6))))
}

/// ReviewThreadCard: its state and comments, folded while resolved, with Reply and Resolve where
/// the host and viewer allow them.
fn thread_card(s: &App, t: &T, th: &Value) -> El {
    let c = &s.ui.pages.code;
    let id = field(th, "id").to_owned();
    let resolved = th["isResolved"] == true;
    let open = !resolved ^ c.flipped.contains(&id);
    let comments = list(Some(th), "comments");
    let count = th["commentCount"].as_u64().map_or(comments.len(), |n| n as usize);
    let muted = t.c.muted_foreground;
    let busy = c.thread_pending.is_some() || !writable(s);
    let flipped = id.clone();
    let state = format!("{} · {count} {}", if resolved { "Resolved" } else { "Open" }, if count == 1 { "comment" } else { "comments" });
    let mut head = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(icon(if resolved { &i::CIRCLE_CHECK } else { &i::CIRCLE }, 14.0).color(if resolved { t.c.success_foreground } else { muted }))
        .child(text(state).font(theme::TEXT_XS).color(muted).cursor(Cursor::Pointer).role("button").expanded(open).on_press(move |s: &mut App| flip(s, &flipped)));
    if th["isOutdated"] == true {
        head = head.child(text("outdated").font(theme::TEXT_XS).color(muted));
    }
    head = head.child(spacer());
    if can_resolve(s, th) {
        let thread = id.clone();
        let label = if resolved { "Unresolve" } else { "Resolve" };
        head = head.child(kit::Button::new(label).variant("ghost").size("xs").disabled(busy).on_press(move |s: &mut App| resolve(s, &thread, !resolved)).build(t));
    }
    let mut card = col().key(format!("thread-{id}")).w_full().padding(12.0).radius(radius::XL).border(1.0, t.c.border.alpha_mul(0.7)).fill(t.c.background).child(head);
    if !open {
        return card;
    }
    let mut thread = col().w_full().gap(12.0).mt(8.0);
    for comment in comments {
        let login = comment["author"]["login"].as_str().unwrap_or("ghost").to_owned();
        let mut meta = row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(avatar(&s.ui.pages.avatars, &comment["author"], 16.0, t))
            .child(text(login).font(theme::XS_MEDIUM).color(t.c.foreground).truncate().min_w(0.0).shrink());
        if let Some(at) = ts(comment, "createdAt") {
            meta = meta.child(text(super::pull_requests::relative(at, s.now_ms)).font(theme::TEXT_XS).color(muted));
        }
        thread = thread.child(col().w_full().min_w(0.0).gap(4.0).child(meta).child(super::pr_detail::markdown(t, field(comment, "body"))));
    }
    card = card.child(thread);
    if !can_reply(s) {
        return card;
    }
    match &c.replying {
        Some((on, edit)) if *on == id => {
            let empty = edit.text.trim().is_empty();
            let input = text_input(edit, theme::TEXT_SM)
                .key("reply-input")
                .role("textbox")
                .label("Reply to this conversation")
                .placeholder("Reply", t.c.contrast_placeholder)
                .w_full()
                .min_h(44.0)
                .color(t.c.foreground)
                .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
                .on_edit(|s: &mut App| &mut s.ui.pages.code.replying.get_or_insert_with(Default::default).1)
                .on_input_key(reply_key);
            let buttons = row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(kit::Button::new("Cancel").variant("ghost").size("xs").on_press(|s: &mut App| s.ui.pages.code.replying = None).build(t))
                .child(kit::Button::new("Reply").size("xs").disabled(busy || empty).on_press(send_reply).build(t));
            card.child(col().w_full().mt(8.0).gap(8.0).child(col().w_full().padding(8.0).radius(8.0).border(1.0, t.c.border).child(input)).child(buttons))
        }
        _ => card.child(row().mt(8.0).child(kit::Button::new("Reply").variant("ghost").size("xs").on_press(move |s: &mut App| start_reply(s, &id)).build(t))),
    }
}

fn row_key(s: &App, i: usize) -> Cow<'_, str> {
    Cow::Borrowed(s.ui.pages.code.diff.keys[i].as_str())
}

/// StyledDiffCodeView: the Diff panel's virtual rows over this tab's state.
fn diff_view(s: &App, t: &T) -> El {
    let st = &s.ui.pages.code.diff;
    let mut look = Look::new(t);
    look.host = Host::PullRequest;
    look.comments = can_select(s);
    let look = Rc::new(look);
    let wrap = st.wraps();
    if !wrap && let Some(r) = rune::memo::layout("pr-code-view/scroll") {
        st.set_viewport(r);
    }
    let mut list = col().key("rows").w_full().growth(GrowthSpec::default()).selection_group(true).role("list").label("Diff");
    if !wrap {
        list = list.min_w(view::content_width(&look, st).max(st.view_width()));
    }
    let l = look.clone();
    let list = list
        .virtual_rows(st.rows.len(), LINE_H, row_key, move |s: &App, i| row_el(s, &l, i))
        .virtual_rows_version(st.keys_rev)
        .on_visible_rows(|s: &mut App, rows| {
            // The sentinel: nearing the last row reads the next page (rootMargin 240px).
            let near_end = rows.end + 12 >= s.ui.pages.code.diff.rows.len();
            let changed = s.ui.pages.code.diff.set_visible_rows(rows);
            (near_end && load_more(s)) || changed
        });
    let scroll = col().key("scroll").w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false });
    let scroll = if wrap { scroll } else { scroll.scroll_xy() };
    let footer = s.ui.pages.code.next.is_some().then(|| page_footer(s, t));
    let mut root = col().key("pr-code-view").focusable(true).grow(1.0).min_w(0.0).min_h(0.0).self_stretch().fill(look.bg).child(scroll.child(list).children(footer));
    if let Some(f) = st.sticky() {
        root = root.child(file_header(s, &look, f, true).key("sticky").absolute(0.0, 0.0).w_full());
    }
    root.on_key(|s: &mut App, e, tx| s.ui.pages.code.diff.key(e, tx))
}

/// renderCodeViewFooter: what became of the next page.
fn page_footer(s: &App, t: &T) -> El {
    let c = &s.ui.pages.code;
    let muted = t.c.muted_foreground;
    let out = row().key("pr-code-more").w_full().py(8.0).gap(8.0).center();
    if c.page_failed {
        out.child(text("The rest of this diff could not be loaded.").font(theme::TEXT_XS).color(muted)).child(kit::Button::new("Retry").variant("outline").size("xs").on_press(retry_page).build(t))
    } else if c.paging.is_some() {
        out.child(text("Loading more files...").font(theme::TEXT_XS).color(muted))
    } else {
        out.h(36.0)
    }
}

fn row_el(s: &App, l: &Look, i: usize) -> El {
    let st = &s.ui.pages.code.diff;
    let key = st.keys[i].as_str();
    let vw = st.view_width().to_bits();
    match st.rows[i] {
        Row::File(f) => file_header(s, l, f as usize, false),
        Row::Note(f) => memo(key, (l.version, st.rev, vw), || view::note(st, l, f as usize)),
        Row::Sep { f, h } => memo(key, (l.version, st.rev, vw), || view::separator(st, l, f as usize, h as usize)),
        Row::Comment { n, .. } => {
            let inner = annotation(s, l, n as usize).key("inner");
            let w = st.view_width();
            row().w_full().child(if !st.wraps() && w > 0.0 { inner.w(w).sticky_left(0.0) } else { inner.w_full() })
        }
        _ => {
            let version = (l.version, st.rev, st.row_selected(i, None), st.selection().map(|s| (s.start.side, s.end.side)), st.hushed, l.comments);
            memo(key, version, || view::line(st, l, i))
        }
    }
}

/// The file's header: fold toggle, change mark, path (renames as old → new), the host's counts
/// and the Viewed box (renderHeaderPrefix and renderHeaderMetadata).
fn file_header(s: &App, l: &Look, f: usize, sticky: bool) -> El {
    let t = &l.t;
    let st = &s.ui.pages.code.diff;
    let file = &st.files()[f];
    let path = file.path().to_owned();
    let collapsed = st.collapsed(f);
    let muted = t.c.muted_foreground;
    let toggled = path.clone();
    let fold = view::micro(l, "toggle", if collapsed { "Expand diff" } else { "Collapse diff" }, if collapsed { &i::CHEVRON_RIGHT } else { &i::CHEVRON_DOWN }, 16.0, muted.alpha_mul(0.8))
        .expanded(!collapsed)
        .ml(-2.0)
        .on_press_tx(move |s: &mut App, tx: &mut Tx| toggle_file(s, &toggled, sticky, tx));
    let (mark, mark_color) = view::change_mark(l, file);
    let mut name = row().key("name").items_center().gap(6.0).min_w(0.0).shrink();
    if let (FileStatus::Renamed | FileStatus::Copied, Some(old)) = (file.status, file.old_path.as_deref()) {
        name = name
            .child(text(old.to_owned()).font(theme::TEXT_XS).color(muted).truncate_head().min_w(0.0).shrink().key("prev"))
            .child(icon(&i::ARROW_RIGHT, 12.0).color(muted).key("arrow"));
    }
    name = name.child(text(path.clone()).font(theme::TEXT_XS).color(t.c.code_foreground).truncate_head().min_w(0.0).shrink().key("title"));
    let (adds, dels) = s.ui.pages.code.counts.get(&path).copied().unwrap_or_else(|| file_counts(file));
    let mut inner = row()
        .key("inner")
        .w_full()
        .h(HEADER_H)
        .pl(8.0)
        .pr(12.0)
        .gap(6.0)
        .items_center()
        .child(fold)
        .child(icon(mark, 16.0).color(mark_color).key("mark"))
        .child(name)
        .child(el().key("space").grow(1.0).a11y_hidden());
    if adds > 0 || dels > 0 {
        let stat = theme::mono(theme::TEXT_2XS).features("'tnum' 1");
        inner = inner.child(
            row()
                .key("stat")
                .gap(4.0)
                .child(text(format!("+{}", crate::chat::details::thousands(adds as u64))).font(stat).color(t.c.diff_addition_foreground))
                .child(text(format!("-{}", crate::chat::details::thousands(dels as u64))).font(stat).color(t.c.diff_deletion_foreground)),
        );
    }
    if viewed_store(s).is_some() {
        inner = inner.child(viewed_toggle(s, t, &path));
    }
    let label = format!("{path}, {adds} added, {dels} removed");
    let mut h = row()
        .w_full()
        .h(HEADER_H)
        .fill(l.bg)
        .hover_group()
        .cursor(Cursor::Pointer)
        .role("heading")
        .label(label)
        .child(inner)
        .on_press_tx(move |s: &mut App, tx: &mut Tx| toggle_file(s, &path, sticky, tx));
    let w = st.view_width();
    if !sticky && !st.wraps() && w > 0.0 {
        h = h.w(w).sticky_left(0.0);
    }
    h
}

/// FileViewedToggle: shift extends the tick from the file pressed last.
fn viewed_toggle(s: &App, t: &T, path: &str) -> El {
    let v = &s.ui.pages.code.viewed;
    let (viewed, changed) = (v.viewed(path), v.changed(path));
    let p = path.to_owned();
    let out = row()
        .key("viewed")
        .w(72.0)
        .gap(6.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .child(kit::Checkbox::new(viewed).hide_label().label(if changed { "Changed" } else { "Viewed" }).build(&kit::theme(t)))
        .on_press_tx(move |s: &mut App, tx: &mut Tx| {
            let shift = tx.mods().shift;
            set_viewed(s, &p, !viewed, shift, tx);
        });
    if changed {
        out.child(text("Changed").font(theme::TEXT_2XS).color(t.c.warning_foreground).help("This file has been pushed to since you marked it viewed."))
    } else {
        out.child(text("Viewed").font(theme::TEXT_2XS).color(t.c.muted_foreground))
    }
}

/// renderAnnotation: the line comment being written, or a pending one (PendingReviewCommentCard).
fn annotation(s: &App, l: &Look, n: usize) -> El {
    let t = &l.t;
    let c = &s.ui.pages.code;
    let Some(note) = c.diff.notes().get(n) else { return col() };
    let wrap = col().w_full().px(12.0).py(6.0).fill(l.bg);
    if note.draft {
        let range = c
            .draft
            .as_ref()
            .and_then(|d| {
                let st = &c.diff;
                let lines = review_lines(st, st.file_index(&d.path)?);
                let end = find_index(&lines, anchor_line(&d.position)?)?;
                let start = match &d.start {
                    Some(p) => find_index(&lines, anchor_line(p)?)?,
                    None => end,
                };
                Some(range_label(&lines[start.min(end)..=start.max(end)]))
            })
            .unwrap_or_else(|| "line".into());
        let empty = c.edit.text.trim().is_empty();
        let review = can_comment(s);
        let input = text_input(&c.edit, theme::TEXT_SM)
            .key("comment-input")
            .role("textbox")
            .label(format!("Comment on lines {range}"))
            .placeholder("Add a comment…", t.c.contrast_placeholder)
            .w_full()
            .min_h(44.0)
            .color(t.c.foreground)
            .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
            .on_edit(|s: &mut App| &mut s.ui.pages.code.edit)
            .on_input_key(editor_key);
        let mut footer = row()
            .w_full()
            .gap(4.0)
            .items_center()
            .child(text("⌘/Ctrl Enter to send").font(theme::TEXT_2XS).color(t.c.muted_foreground.alpha_mul(0.7)).grow(1.0).min_w(0.0).truncate())
            .child(kit::Button::new("Cancel").variant("ghost-muted").size("xs").on_press(cancel).build(t));
        if review {
            footer = footer.child(kit::Button::new("Add to agent").variant("outline").size("xs").disabled(empty).on_press(add_to_agent).build(t));
        }
        footer = footer.child(kit::Button::new(if review { "Add to review" } else { "Add to agent" }).size("xs").disabled(empty).on_press(submit_draft).build(t));
        return wrap.key("comment-draft").child(col().w_full().gap(6.0).padding(8.0).radius(8.0).border(1.0, t.c.border).fill(t.c.background).child(input).child(footer));
    }
    if let Some(id) = note.id.strip_prefix(THREAD) {
        let Some(th) = threads(s).iter().find(|th| field(th, "id") == id) else { return col() };
        return wrap.key("thread").child(thread_card(s, t, th));
    }
    let key = s.ui.pages.detail.entry.as_ref().map(review_key);
    let comment = key.and_then(|k| c.pending.get(&k)).and_then(|list| list.iter().find(|p| field(p, "id") == note.id));
    let body = comment.map_or(String::new(), |p| field(p, "body").to_owned());
    let id = note.id.clone();
    let head = row()
        .w_full()
        .gap(8.0)
        .center_y()
        .child(icon(&i::MESSAGE_SQUARE, 14.0).color(t.c.muted_foreground))
        .child(text("Pending. Sent when you submit the review").font(theme::TEXT_XS).color(t.c.muted_foreground).grow(1.0).min_w(0.0).truncate())
        .child(kit::Button::new("Discard this comment").icon(&i::TRASH_2).variant("ghost").size("icon-xs").on_press(move |s: &mut App| discard(s, &id)).build(t).help("Discard this comment"));
    wrap.key("comment").child(
        col()
            .w_full()
            .gap(8.0)
            .padding(12.0)
            .radius(radius::XL)
            .border(1.0, t.c.border.alpha_mul(0.7))
            .fill(t.c.background)
            .child(head)
            .child(text(body).font(theme::TEXT_SM).color(t.c.foreground).wrap()),
    )
}

/// DiffFileTree: the files by folder; pressing one opens it and scrolls to it.
fn tree(s: &App, t: &T) -> El {
    let c = &s.ui.pages.code;
    let st = &c.diff;
    let u = &s.ui.pages.detail;
    let d = u.detail.as_ref().or(u.entry.as_ref()).unwrap_or(&Value::Null);
    let paths: Vec<&str> = st.files().iter().map(FileDiff::path).collect();
    let rows = tree_rows(&paths, &c.folded_dirs);
    let dirs = all_dirs(&paths);
    let current = st.visible_file();
    let mut head = row()
        .h(40.0)
        .px(8.0)
        .gap(4.0)
        .center_y()
        .w_full()
        .child(text("Files").font(theme::XS_MEDIUM).color(t.c.foreground).px(4.0))
        .child(spacer())
        .child(text(paths.len().to_string()).font(theme::TEXT_XS.features("'tnum' 1")).color(t.c.muted_foreground));
    if !dirs.is_empty() {
        let open = dirs.iter().all(|d| !c.folded_dirs.contains(d));
        let label = if open { "Collapse all folders" } else { "Expand all folders" };
        let fold_all = kit::Button::new(label)
            .icon(if open { &i::CHEVRONS_DOWN_UP } else { &i::CHEVRONS_UP_DOWN })
            .variant("ghost")
            .size("icon-xs")
            .on_press(move |s: &mut App| {
                let c = &mut s.ui.pages.code;
                if open {
                    let paths: Vec<&str> = c.diff.files().iter().map(FileDiff::path).collect();
                    c.folded_dirs = all_dirs(&paths);
                } else {
                    c.folded_dirs.clear();
                }
            });
        head = head.child(fold_all.build(t).help(label));
    }
    let name = format!("{} #{} files", super::pr_row::capitalized(super::pr_row::term(d)), d["number"]);
    let mut list = col().key("pr-code-tree-rows").w_full().padding(4.0).gap(1.0).role("tree").label(name);
    for (n, r) in rows.into_iter().enumerate() {
        let pad = 8.0 + r.depth as f32 * 12.0;
        let base = row().key(format!("t{n}")).h(24.0).pl(pad).pr(8.0).gap(6.0).radius(6.0).center_y().w_full().cursor(Cursor::Pointer).focusable(true).role("treeitem");
        let line = if let Some(dir) = r.dir {
            let open = !c.folded_dirs.contains(&dir);
            base.hover(t.c.foreground.alpha(0.07))
                .label(r.label.clone())
                .expanded(open)
                .child(icon(if open { &i::CHEVRON_DOWN } else { &i::CHEVRON_RIGHT }, 12.0).color(t.c.muted_foreground))
                .child(icon(if open { &i::FOLDER_OPEN } else { &i::FOLDER }, 14.0).color(t.c.muted_foreground))
                .child(text(r.label).font(theme::TEXT_XS).color(t.c.foreground).single_line().truncate().min_w(0.0).shrink())
                .on_press(move |s: &mut App| {
                    let folded = &mut s.ui.pages.code.folded_dirs;
                    if !folded.remove(&dir) {
                        folded.insert(dir.clone());
                    }
                })
        } else {
            let f = r.file.unwrap_or(0);
            let file = &st.files()[f];
            let color = match file.status {
                FileStatus::Added | FileStatus::Copied => t.c.diff_addition_foreground,
                FileStatus::Deleted => t.c.diff_deletion_foreground,
                FileStatus::Renamed => t.c.diff_modified_icon.alpha(1.0),
                FileStatus::Modified => t.c.foreground,
            };
            let selected = f == current;
            let path = file.path().to_owned();
            base.pl(pad + 18.0)
                .fill(if selected { t.c.foreground.alpha(0.12) } else { Color::TRANSPARENT })
                .hover(t.c.foreground.alpha(if selected { 0.12 } else { 0.07 }))
                .label(path.clone())
                .child(icon(&i::FILE, 14.0).color(t.c.muted_foreground))
                .child(text(r.label).font(theme::TEXT_XS).color(color).single_line().truncate().min_w(0.0).shrink())
                .on_press_tx(move |s: &mut App, tx: &mut Tx| reveal(s, &path, tx))
        };
        list = list.child(line);
    }
    let out = col()
        .key("pr-code-tree")
        .w_frac(0.4)
        .max_w(320.0)
        .min_h(0.0)
        .self_stretch()
        .child(head)
        .child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.6)))
        .child(col().grow(1.0).min_h(0.0).w_full().child(list).scroll(ScrollSpec { stick_bottom: false }));
    if c.next.is_none() {
        return out;
    }
    let label = if c.page_failed {
        "Retry"
    } else if c.paging.is_some() {
        "Loading more files..."
    } else {
        "Load more files"
    };
    let more = col().w_full().items_stretch().child(kit::Button::new(label).variant("outline").size("xs").disabled(c.paging.is_some()).on_press(retry_page).build(t));
    out.child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.6))).child(more.key("pr-code-tree-more").padding(8.0))
}

// ---- scenes -----------------------------------------------------------------------------

/// Fixture states for the Code tab: `tree` (file tree and viewed ticks), `split` (side by side),
/// `threads` (a conversation inline, and two no line holds, opened), `options` (the narrow
/// toolbar's menu open), `more-files` (a next page to read), `pending-review` (two comments
/// pending, for the composer) and `line-comment` (a pending comment and a ranged draft).
pub(super) fn scene(s: &mut App, kind: &str) {
    let usage = "apps/desktop-rune/src/pages/usage.rs";
    let pending = |n: u32, line: u32, body: &str| json!({"id": format!("pending-review-comment-{n}"), "path": usage, "position": {"kind": "added", "newLine": line}, "body": body});
    match kind {
        "threads" => {
            let author = |login: &str, name: &str| json!({"login": login, "name": name, "avatarUrl": null});
            if let Some(list) = s.ui.pages.detail.activity.as_mut().and_then(|a| a["reviewThreads"].as_array_mut()) {
                list.push(json!({"id": "t2", "path": "apps/desktop-rune/src/main.rs", "line": 4, "side": "right", "isResolved": false, "isOutdated": true, "comments": [
                    {"id": "tc3", "author": author("theo", "Theo"), "body": "Does anything still route through the old Usage arm?", "createdAt": "2026-11-06T08:40:00.000Z", "url": null},
                ]}));
                list.push(json!({"id": "t3", "path": "apps/desktop-rune/src/pages/usage_limits.rs", "line": null, "side": "right", "isResolved": true, "isOutdated": true, "comments": [
                    {"id": "tc4", "author": author("juliusmarminge", "Julius Marminge"), "body": "Split the limits card out of usage.rs.", "createdAt": "2026-11-06T08:52:00.000Z", "url": null},
                ]}));
            }
            if let Some(d) = &mut s.ui.pages.detail.detail {
                d["capabilities"]["review"]["reply"] = json!(true);
                d["capabilities"]["review"]["resolve"] = json!(true);
                d["viewerPermissions"]["comment"] = json!(true);
                d["viewerPermissions"]["resolve"] = json!(true);
            }
            unfold(s, usage);
            s.ui.pages.code.orphans_open = true;
            sync_notes(s);
        }
        "options" => {
            unfold(s, usage);
            s.ui.pages.code.options = true;
        }
        "more-files" => {
            s.client_settings.values.insert(TREE_KEY.into(), json!(true));
            s.ui.pages.code.next = Some("page-2".into());
            unfold(s, usage);
        }
        "pending-review" => {
            if let Some(key) = s.ui.pages.detail.entry.as_ref().map(review_key) {
                let list = vec![pending(1, 210, "Name this `wall_start` so it reads apart from `start`."), pending(2, 211, "Does hour_label follow the 12-hour setting here?")];
                s.ui.pages.code.pending.insert(key, list);
            }
            sync_notes(s);
        }
        "tree" => {
            s.client_settings.values.insert(TREE_KEY.into(), json!(true));
            if let Some(d) = &mut s.ui.pages.detail.detail {
                d["capabilities"]["viewedFiles"] = json!("environment");
            }
            if let Some(entry) = s.ui.pages.detail.entry.clone() {
                let v = &mut s.ui.pages.code.viewed;
                v.key = viewed_key(&entry);
                v.entry = Some(entry);
                v.answer = Some(json!({"files": [
                    {"path": "apps/desktop-rune/src/pages/mod.rs", "state": "viewed"},
                    {"path": "apps/desktop-rune/src/main.rs", "state": "dismissed"},
                ], "truncated": false}));
            }
            let default = fold_default(s);
            fold_to(&mut s.ui.pages.code, default, "apps/desktop-rune/src/pages/mod.rs", true);
            unfold(s, usage);
        }
        "split" => {
            s.client_settings.values.insert("diffLayout".into(), json!("split"));
            s.ui.pages.code.diff.set_mode(DiffMode::Split);
            unfold(s, usage);
            unfold(s, "apps/desktop-rune/src/main.rs");
        }
        _ => {
            if let Some(d) = &mut s.ui.pages.detail.detail {
                d["capabilities"]["review"]["inlineComment"] = json!(true);
                d["capabilities"]["review"]["rangeComment"] = json!(true);
            }
            let Some(f) = unfold(s, usage) else { return };
            let line = |n: u32| LineRef { side: Side::New, line: n };
            begin(s, f, line(210));
            s.ui.pages.code.edit = TextEdit::new("Name this `wall_start` so it reads apart from `start`.");
            add_to_review(s);
            s.ui.pages.code.diff.set_selection(Some(LineRange { file: f, start: line(210), end: line(211) }));
            begin(s, f, line(211));
            s.ui.pages.code.edit = TextEdit::new("Does hour_label follow the 12-hour setting here?");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch() -> Vec<FileDiff> {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/fixtures/pull-request-detail.json");
        let recorded: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
        rune_kit::diff::parse(recorded["diff"]["patch"].as_str().unwrap())
    }

    fn at(side: Side, line: u32) -> LineRef {
        LineRef { side, line }
    }

    #[test]
    fn the_file_tree_folds_folders_and_a_reveal_only_ever_opens() {
        let files = patch();
        let paths: Vec<&str> = files.iter().map(FileDiff::path).collect();
        let labels = |folded: &HashSet<String>| tree_rows(&paths, folded).into_iter().map(|r| (r.depth, r.label)).collect::<Vec<_>>();
        assert_eq!(labels(&HashSet::new()), [(0, "apps/desktop-rune/src".to_string()), (1, "pages".into()), (2, "mod.rs".into()), (2, "usage.rs".into()), (1, "main.rs".into())]);
        let folded = HashSet::from(["apps/desktop-rune/src/pages".to_string()]);
        assert_eq!(labels(&folded), [(0, "apps/desktop-rune/src".to_string()), (1, "pages".into()), (1, "main.rs".into())]);
        assert_eq!(all_dirs(&paths).len(), 2, "Collapse all folders folds both");

        let mut s = App::new();
        s.client_settings.values.insert("diffFilesCollapsed".into(), json!(true));
        s.ui.pages.code.diff.set_files(files.clone(), &paths.iter().map(|p| p.to_string()).collect::<Vec<_>>());
        let usage = "apps/desktop-rune/src/pages/usage.rs";
        assert_eq!(unfold(&mut s, usage), Some(1));
        assert!(!s.ui.pages.code.diff.collapsed(1));
        unfold(&mut s, usage);
        assert!(!s.ui.pages.code.diff.collapsed(1), "a second reveal must not fold it again");
        assert_eq!(s.ui.pages.code.toggled, HashSet::from([usage.to_string()]));
        toggle_all(&mut s);
        assert!(s.ui.pages.code.diff.all_collapsed() && s.ui.pages.code.toggled.is_empty() && s.ui.pages.code.fold == Some(true));
        toggle_all(&mut s);
        assert!((0..3).all(|f| !s.ui.pages.code.diff.collapsed(f)));
        assert!(is_collapsed(usage, false, &HashSet::from([usage.to_string()])));
    }

    #[test]
    fn viewed_ticks_are_kept_per_pull_request_and_environment() {
        let entry = json!({"projectId": "p", "host": "GitHub.com", "repository": "Acme/App", "number": 7, "url": "u"});
        assert_eq!(viewed_key(&entry), r#"[null,"p","GitHub.com",null,null,"Acme/App",7]"#);
        let mut other = entry.clone();
        other[TAG] = json!("remote");
        assert_ne!(viewed_key(&entry), viewed_key(&other), "two environments can share a project id");
        assert_eq!(review_key(&entry), r#"["p","github.com","acme/app",7]"#);
        let input = set_viewed_input(&entry, &[("a.rs".to_string(), true), ("b.rs".to_string(), false)]);
        assert_eq!(input, json!({"projectId": "p", "host": "GitHub.com", "repository": "Acme/App", "number": 7, "files": [{"path": "a.rs", "viewed": true}, {"path": "b.rs", "viewed": false}]}));

        let mut v = Viewed { answer: Some(json!({"files": [{"path": "a.rs", "state": "viewed"}, {"path": "c.rs", "state": "dismissed"}], "truncated": false})), ..Viewed::default() };
        assert!(v.viewed("a.rs") && !v.viewed("b.rs") && v.changed("c.rs"));
        v.press("b.rs", true, 1000);
        v.press("c.rs", true, 1000);
        assert!(v.viewed("b.rs") && !v.changed("c.rs"), "a press stands over the answer");
        assert_eq!(v.take_batch(1399), None, "the debounce holds");
        let batch = v.take_batch(1400).unwrap();
        assert_eq!(batch, [("b.rs".to_string(), true), ("c.rs".to_string(), true)]);
        v.sending = Some((1, batch.clone()));
        v.settle(json!({"files": []}), 0);
        assert!(v.viewed("b.rs"), "an answer that predates the write keeps the press");
        v.sending = None;
        v.landed.insert("b.rs".into(), 1);
        v.landed.insert("c.rs".into(), 1);
        v.settle(json!({"files": [{"path": "c.rs", "state": "dismissed"}]}), 1);
        assert!(!v.viewed("b.rs") && v.changed("c.rs"), "a read begun after the write answers for it");
        v.press("d.rs", true, 0);
        v.queued.clear();
        assert!(v.failed(&[("d.rs".to_string(), true)]) && !v.viewed("d.rs"), "a failed write reverts its press");

        let paths = ["a".to_string(), "b".into(), "c".into(), "d".into()];
        assert_eq!(viewed_range(&paths, "b", Some("d")), &paths[1..4]);
        assert_eq!(viewed_range(&paths, "c", Some("gone")), &paths[2..3]);
        assert!(viewed_range(&paths, "gone", None).is_empty());
    }

    #[test]
    fn the_layout_follows_the_shared_setting_and_wrap_is_the_tabs_own() {
        let mut s = App::new();
        s.client_settings.values.insert("diffLayout".into(), json!("split"));
        assert_eq!(layout(&s), DiffMode::Split);
        s.client_settings.values.insert("diffLayout".into(), json!("stacked"));
        assert_eq!(layout(&s), DiffMode::Unified);
        s.client_settings.values.insert("wordWrap".into(), json!(false));
        assert!(!wrap_on(&s));
        s.ui.pages.code.wrap = Some(true);
        assert!(wrap_on(&s), "the toggle wins over the setting it was seeded from");
        s.client_settings.values.remove(TREE_KEY);
        assert!(!tree_open(&s), "the file tree starts hidden");
    }

    #[test]
    fn a_line_comment_carries_the_hosts_position_and_its_first_line_in_one_hunk() {
        let files = patch();
        let st = DiffState::new(files.clone());
        let usage = &files[1];
        let lines = review_lines(&st, 1);
        // 208 context, 209 context, -210, +210, +211, then context.
        let added = anchor(usage, &lines, at(Side::New, 210), at(Side::New, 211), true).unwrap();
        assert_eq!(added, (json!({"kind": "added", "newLine": 211}), Some(json!({"kind": "added", "newLine": 210}))));
        assert_eq!(anchor(usage, &lines, at(Side::New, 211), at(Side::New, 210), true).unwrap().1, Some(json!({"kind": "added", "newLine": 210})), "diff order, not press order");
        assert_eq!(anchor(usage, &lines, at(Side::New, 210), at(Side::New, 211), false).unwrap(), (json!({"kind": "added", "newLine": 211}), None));
        assert_eq!(anchor(usage, &lines, at(Side::Old, 210), at(Side::Old, 210), true).unwrap().0, json!({"kind": "deleted", "oldLine": 210}));
        assert_eq!(anchor(usage, &lines, at(Side::Old, 208), at(Side::Old, 208), false).unwrap().0, json!({"kind": "context", "oldLine": 208, "newLine": 208, "side": "left"}));
        assert_eq!(anchor(usage, &lines, at(Side::New, 400), at(Side::New, 400), false), None);
        assert_eq!(anchor_line(&json!({"kind": "context", "oldLine": 208, "newLine": 208, "side": "left"})), Some(at(Side::Old, 208)));

        let draft = Draft { path: "b.rs".into(), old_path: Some("a.rs".into()), position: added.0.clone(), start: added.1.clone(), range: (at(Side::New, 210), at(Side::New, 211)) };
        assert_eq!(
            pending_comment("pending-review-comment-1".into(), &draft, "Rename this."),
            json!({"id": "pending-review-comment-1", "path": "b.rs", "oldPath": "a.rs", "position": {"kind": "added", "newLine": 211}, "startPosition": {"kind": "added", "newLine": 210}, "body": "Rename this."})
        );
    }

    /// An app showing the recorded diff of `entry`, whose host takes line comments and threads.
    fn showing(entry: &Value) -> App {
        let mut s = App::new();
        s.ui.pages.detail.entry = Some(entry.clone());
        s.ui.pages.detail.detail = Some(json!({"number": 7, "capabilities": {"review": {"inlineComment": true}}, "viewerPermissions": {"comment": true}}));
        s.ui.pages.detail.files = Some(patch());
        s.client_settings.values.insert("diffIgnoreWhitespace".into(), json!(false));
        show(&mut s);
        s
    }

    #[test]
    fn a_landed_review_takes_the_comments_it_carried_and_a_refused_one_keeps_them() {
        let entry = json!({"projectId": "p", "host": "github.com", "repository": "o/r", "number": 7});
        let mut s = showing(&entry);
        let comment = |n: u32| json!({"id": format!("pending-review-comment-{n}"), "path": "apps/desktop-rune/src/pages/usage.rs", "position": {"kind": "added", "newLine": 210}, "body": "x"});
        s.ui.pages.code.pending.insert(review_key(&entry), vec![comment(1), comment(2)]);
        let sent = pending(&s, &entry).to_vec();
        hold_submitted(&mut s, &sent);
        assert_eq!(pending(&s, &entry).len(), 2, "a refused review leaves its comments to send again");
        hold_submitted(&mut s, &sent);
        s.ui.pages.code.pending.get_mut(&review_key(&entry)).unwrap().push(comment(3));
        review_submitted(&mut s);
        assert_eq!(pending(&s, &entry), [comment(3)], "one written while it was sending stays");
        review_submitted(&mut s);
        assert_eq!(pending(&s, &entry).len(), 1, "a second landing has nothing of its own to take");
        discard_all(&mut s);
        assert!(pending(&s, &entry).is_empty() && s.ui.pages.code.pending.is_empty());
    }

    #[test]
    fn hiding_whitespace_reads_a_reindented_line_as_unchanged() {
        let patch = "diff --git a/a.rs b/a.rs\n--- a/a.rs\n+++ b/a.rs\n@@ -1,4 +1,4 @@\n fn f() {\n-  let a = 1;\n-  call(a);\n+    let a = 1;\n+    call(b);\n }\n";
        let file = &rune_kit::diff::parse(patch)[0];
        let shown = hide_whitespace(file);
        let lines: Vec<_> = shown.hunks[0].lines.iter().map(|l| (l.kind, l.old, l.new, l.text.as_str())).collect();
        assert_eq!(
            lines,
            [
                (LineKind::Context, Some(1), Some(1), "fn f() {"),
                (LineKind::Context, Some(2), Some(2), "    let a = 1;"),
                (LineKind::Removed, Some(3), None, "  call(a);"),
                (LineKind::Added, None, Some(3), "    call(b);"),
                (LineKind::Context, Some(4), Some(4), "}"),
            ]
        );
        assert_eq!(hide_whitespace(&shown).hunks, shown.hunks, "nothing left to hide");

        let entry = json!({"projectId": "p", "host": "github.com", "repository": "o/r", "number": 7});
        let mut s = showing(&entry);
        s.ui.pages.detail.files = Some(rune_kit::diff::parse(patch));
        set_ignore_ws(&mut s, true);
        assert_eq!(s.ui.pages.code.counts.get("a.rs"), Some(&(2, 2)), "the header keeps the host's counts");
        assert_eq!(s.ui.pages.code.diff.files()[0].additions(), 1);
        begin(&mut s, 0, at(Side::New, 2));
        assert_eq!(s.ui.pages.code.draft.as_ref().map(|d| d.position.clone()), Some(json!({"kind": "added", "newLine": 2})), "a comment anchors where the host's diff has the line");
        set_ignore_ws(&mut s, false);
        assert!(s.ui.pages.code.draft.is_none(), "a draft's lines may not survive the switch");

        let two = rune_kit::diff::parse(&format!("{patch}{}", patch.replace("a.rs", "a.test.ts")));
        let paths = |files: Vec<FileDiff>| files.iter().map(|f| f.path().to_string()).collect::<Vec<_>>();
        assert_eq!(paths(shown_files(&two, true, false, None)), ["a.rs"]);
        assert_eq!(paths(shown_files(&two, true, false, Some("a.test.ts"))), ["a.rs", "a.test.ts"], "a revealed test stays");
        assert_eq!(paths(shown_files(&two, false, false, None)).len(), 2);
    }

    #[test]
    fn threads_show_at_their_line_and_the_rest_are_listed_apart() {
        let entry = json!({"projectId": "p", "host": "github.com", "repository": "o/r", "number": 7});
        let mut s = showing(&entry);
        let thread = |id: &str, path: &str, line: Value, side: &str| json!({"id": id, "path": path, "line": line, "side": side, "isResolved": false, "isOutdated": false, "comments": []});
        s.ui.pages.detail.activity = Some(json!({"reviewThreads": [
            thread("held", "apps/desktop-rune/src/pages/usage.rs", json!(212), "right"),
            thread("old-side", "apps/desktop-rune/src/pages/usage.rs", json!(210), "left"),
            thread("outside", "apps/desktop-rune/src/main.rs", json!(4), "right"),
            thread("no-line", "apps/desktop-rune/src/main.rs", Value::Null, "right"),
            thread("gone", "deleted.rs", json!(1), "right"),
        ]}));
        sync_notes(&mut s);
        let notes: Vec<(String, LineRef)> = s.ui.pages.code.diff.notes().iter().map(|n| (n.id.clone(), n.at)).collect();
        assert_eq!(notes, [("thread:held".to_string(), at(Side::New, 212)), ("thread:old-side".to_string(), at(Side::Old, 210))]);
        let orphans = |s: &App| orphan_threads(s).iter().map(|th| field(th, "id").to_string()).collect::<Vec<_>>();
        assert_eq!(orphans(&s), ["outside", "no-line", "gone"]);
        s.ui.pages.detail.commit = Some("abc".into());
        s.ui.pages.detail.activity.as_mut().unwrap()["commits"] = json!([{"oid": "abc"}]);
        assert_eq!(orphans(&s).len(), 5, "a commit's lines are another diff's, so every thread is listed apart");
    }

    #[test]
    fn a_selection_goes_to_the_agent_as_the_lines_with_an_empty_note() {
        let entry = json!({"projectId": "p", "host": "github.com", "repository": "o/r", "number": 7});
        let s = showing(&entry);
        let usage = "apps/desktop-rune/src/pages/usage.rs";
        let d = Draft { path: usage.into(), old_path: None, position: json!({"kind": "added", "newLine": 211}), start: None, range: (at(Side::New, 210), at(Side::New, 211)) };
        let c = selection_context(&s.ui.pages.code.diff, &d, 7, "Why two lines?").unwrap();
        assert_eq!((c["sectionId"].as_str(), c["sectionTitle"].as_str(), c["text"].as_str()), (Some("pull-request:7"), Some("PR #7 review"), Some("")));
        assert_eq!(c["contextId"], json!(crate::chat::composer::chips::context_id("review-comment", &format!("pull-request-selection:{usage}:210:211"))));
        assert_eq!(c["rangeLabel"], "+210 to +211");
    }
}
