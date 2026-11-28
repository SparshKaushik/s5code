//! The pull request detail's Code tab (PullRequestCodeTab): the toolbar over the diff, files
//! folded and ticked viewed, the file tree, and line comments pending the review. The rows are
//! the Diff panel's (`panel::diff::view`), drawn over this tab's own `DiffState`.

use std::borrow::Cow;
use std::cell::Cell;
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
use super::pr_edit::Draft as EditDraft;
use crate::App;
use crate::data::{s as field, ts};
use crate::env::TAG;
use crate::icons as i;
use crate::kit::{self, Menu, MenuItem, Placement};
use crate::panel::diff::comments::{ReviewLine, find_index, range_label, record, review_lines};
use crate::panel::diff::find::{self as diff_find, Find};
use crate::panel::diff::state::{DiffMode, DiffState, HEADER_H, LINE_H, LineRange, LineRef, Note, Row};
use crate::panel::diff::view::{self, Host, Look};
use crate::panel::diff::{all_dirs, is_test_file, layout_toggle, save_layout, toggle, tree_rows};
use crate::theme::{self, AlphaMul, T, radius};

/// filesViewedStore's `flushDelayMs`.
const FLUSH_MS: i64 = 400;
/// The web keeps this in localStorage (`t3code.pullRequestFileTreeOpen`).
const TREE_KEY: &str = "pullRequestFileTreeOpen";
/// The web keeps this in localStorage (`t3code.diffHideTestFiles`).
use crate::panel::diff::HIDE_TESTS_KEY;
const DRAFT: &str = "draft";
/// Prefixes a review thread's note id.
const THREAD: &str = "thread:";
/// The narrowest toolbar that keeps its view toggles out of the Diff options menu (`@lg`).
const WIDE_TOOLBAR: f32 = 512.0;
/// Past this many removed-by-added line pairs a change keeps its whitespace-only lines.
const WS_CELLS: usize = 1 << 20;
const REPLY_FAILED: &str = "Reply could not be posted";
const EDIT_FAILED: &str = "The comment could not be saved";
const COMMENTS_FAILED: &str = "More comments could not be loaded";
/// The web keeps this in localStorage (`t3code:diff-file-tree-width`).
const TREE_WIDTH_KEY: &str = "diffFileTreeWidth";
/// PullRequestCommentBody's clip (`max-h-[240px]`).
const COMMENT_CLIP: f32 = 240.0;
/// getRenderablePatch's reason for a page it read no file from.
const RAW_REASON: &str = "Unsupported diff format. Showing raw patch.";
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
    /// How many files each page read past the first held, since each page is ordered apart.
    pages: Vec<usize>,
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
    /// Whether the toolbar measured under `WIDE_TOOLBAR` when it was last shown.
    narrow: Cell<Option<bool>>,
    /// Comments read past a thread's first page and the cursor after them, by thread (loadedPage).
    pub(super) loaded: HashMap<String, (Vec<Value>, Option<String>)>,
    /// Comment pages in flight: the request, its thread and the scope it was asked for.
    comment_pages: Vec<(u64, String, String)>,
    /// The review thread comment being edited.
    editing: Option<(String, EditDraft)>,
    /// The file tree's row width as last laid out, and the width being dragged.
    tree_row: Cell<f32>,
    tree_drag: Option<f32>,
    /// Comments shown past PullRequestCommentBody's clip.
    full_comments: HashSet<String>,
    /// The file last revealed from outside the tab, shown even while tests are hidden (revealRequest).
    revealed: Option<String>,
    /// That reveal until the file is loaded and scrolled to, paging forward meanwhile.
    reveal: Option<String>,
    /// Find in the diff (`panel::diff::find`), as the Diff panel has it.
    pub(crate) find: Option<Find>,
    pub(crate) find_edit: TextEdit,
    /// A finished turn's reread of the diff's first page, and the scope it was asked for.
    refetch: Option<(u64, String)>,
    /// The shown first page's `page_mark`, which a reread compares against.
    pub(super) first_mark: Option<u64>,
    /// Counts the host gave for files whose content it withheld, by path (omittedFileStats).
    pub(super) omitted: HashMap<String, (usize, usize)>,
    /// Pages the parser read no file from, shown as text (getRenderablePatch's raw kind).
    pub(super) raw: Vec<String>,
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
    crate::panel::diff::hide_test_files(s)
}

/// The files shown: tests dropped while hidden, except one a reveal asked for, and whitespace-only
/// changes read as unchanged while hidden.
fn shown_files<'a>(source: impl IntoIterator<Item = &'a FileDiff>, hide: bool, ws: bool, kept: Option<&str>) -> Vec<FileDiff> {
    source.into_iter().filter(|f| shown(f, hide, kept)).map(|f| if ws { hide_whitespace(f) } else { f.clone() }).collect()
}

fn shown(f: &FileDiff, hide: bool, kept: Option<&str>) -> bool {
    !hide || !is_test_file(f.path()) || kept == Some(f.path())
}

/// The tab's "no file changes" state: nothing survives the test filter, no page is left and
/// none was left unread.
pub(super) fn no_changes(s: &App, files: &[FileDiff]) -> bool {
    let kept = s.ui.pages.code.revealed.as_deref();
    s.ui.pages.code.next.is_none() && s.ui.pages.code.raw.is_empty() && !files.iter().any(|f| shown(f, hide_tests(s), kept))
}

/// The diff in web's order (diffModel): each page read by `order_diff_files`, then all of them as
/// the file tree lists them. `pages` holds the length of each page read after the first.
fn ordered_files<'a>(source: &'a [FileDiff], pages: &[usize]) -> Vec<&'a FileDiff> {
    let mut sliced = Vec::with_capacity(source.len());
    let mut rest = source;
    for len in std::iter::once(source.len().saturating_sub(pages.iter().sum::<usize>())).chain(pages.iter().copied()) {
        let (page, after) = rest.split_at(len.min(rest.len()));
        let files: Vec<(&str, Vec<&str>)> = page
            .iter()
            .map(|f| {
                let lines: Vec<&str> = if tier(f.path()) == Tier::Source { f.hunks.iter().flat_map(|h| &h.lines).map(|l| l.text.as_str()).collect() } else { vec![] };
                (f.path(), lines)
            })
            .collect();
        sliced.extend(order_diff_files(&files).into_iter().map(|n| &page[n]));
        rest = after;
    }
    let paths: Vec<&str> = sliced.iter().map(|f| f.path()).collect();
    tree_order(&paths).into_iter().map(|n| sliced[n]).collect()
}

/// DiffFileTier.
#[derive(Clone, Copy, PartialEq)]
enum Tier {
    Source,
    Test,
    Generated,
}

fn tier(path: &str) -> Tier {
    const GENERATED_FILE_NAMES: &[&str] = &["pnpm-lock.yaml", "yarn.lock", "package-lock.json", "bun.lockb", "Cargo.lock", "go.sum", "composer.lock", "Gemfile.lock"];
    const GENERATED_DIRECTORIES: &[&str] = &["__snapshots__", "__generated__", "dist", "build", "vendor"];
    const TEST_DIRECTORIES: &[&str] = &["__tests__", "tests", "test"];
    let (dirs, name) = path.rsplit_once('/').map_or((None, path), |(d, n)| (Some(d), n));
    let under = |set: &[&str]| dirs.is_some_and(|d| d.split('/').any(|s| set.contains(&s)));
    if GENERATED_FILE_NAMES.contains(&name) || name.ends_with(".snap") || name.ends_with(".min.js") || name.ends_with(".min.css") || name.contains(".generated.") || under(GENERATED_DIRECTORIES) {
        Tier::Generated
    } else if name.contains(".test.") || name.contains(".spec.") || under(TEST_DIRECTORIES) {
        Tier::Test
    } else {
        Tier::Source
    }
}

/// `localeCompare` closely enough for paths: case folded first.
fn locale_cmp(a: &str, b: &str) -> std::cmp::Ordering {
    a.to_lowercase().cmp(&b.to_lowercase()).then_with(|| a.cmp(b))
}

fn strip_extension(path: &str) -> &str {
    match (path.rfind('.'), path.rfind('/')) {
        (Some(dot), slash) if slash.is_none_or(|slash| dot > slash) => &path[..dot],
        _ => path,
    }
}

fn base_name(path: &str) -> &str {
    strip_extension(path.rsplit('/').next().unwrap_or(path))
}

fn resolve_relative(from: &str, specifier: &str) -> String {
    let mut segments: Vec<&str> = from.split('/').collect();
    segments.pop();
    for part in specifier.split('/') {
        match part {
            "." | "" => {}
            ".." => {
                segments.pop();
            }
            _ => segments.push(part),
        }
    }
    segments.join("/")
}

/// IMPORT_SPECIFIER's captures in one line: what `from`, `import` or `require(` names in quotes.
fn specifiers(line: &str) -> Vec<&str> {
    let b = line.as_bytes();
    let word = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    let skip = |mut at: usize| {
        while at < b.len() && matches!(b[at], b' ' | b'\t' | b'\n' | b'\r' | 0x0B | 0x0C) {
            at += 1;
        }
        at
    };
    let quoted = |at: usize| {
        let mut at = skip(at);
        if b.get(at) == Some(&b'(') {
            at = skip(at + 1);
        }
        if !matches!(b.get(at), Some(b'"' | b'\'')) {
            return None;
        }
        let start = at + 1;
        let end = start + b[start..].iter().position(|&c| c == b'"' || c == b'\'')?;
        (end > start).then_some((start, end))
    };
    let mut out = vec![];
    let mut at = 0;
    while at < b.len() {
        let after = if at > 0 && word(b[at - 1]) {
            None
        } else if b[at..].starts_with(b"from") {
            Some(at + 4)
        } else if b[at..].starts_with(b"import") {
            Some(at + 6)
        } else if b[at..].starts_with(b"require") {
            let open = skip(at + 7);
            (b.get(open) == Some(&b'(')).then_some(open + 1)
        } else {
            None
        };
        match after.and_then(quoted) {
            Some((start, end)) => {
                out.push(&line[start..end]);
                at = end + 1;
            }
            None => at += 1,
        }
    }
    out
}

fn imported_paths<'a>(path: &str, lines: &[&str], by_module: &HashMap<&str, &'a str>, by_base: &HashMap<&str, &'a str>) -> HashSet<&'a str> {
    const MODULE_EXTENSIONS: &[&str] = &[".ts", ".tsx", ".js", ".jsx", ".mjs", ".cjs"];
    let mut imported = HashSet::new();
    for line in lines {
        for specifier in specifiers(line) {
            let with_extension = if specifier.starts_with('.') { resolve_relative(path, specifier) } else { specifier.to_owned() };
            // Module-map keys omit extensions, which can differ from disk.
            let resolved = MODULE_EXTENSIONS.iter().find_map(|e| with_extension.strip_suffix(*e)).unwrap_or(with_extension.as_str());
            let target = by_module.get(resolved).or_else(|| by_module.get(format!("{resolved}/index").as_str())).or_else(|| by_base.get(base_name(resolved)));
            if let Some(&target) = target
                && target != path
            {
                imported.insert(target);
            }
        }
    }
    imported
}

/// orderByImports: whatever a file imports comes before it, by path where that cannot hold.
fn order_by_imports<'a>(paths: &[&'a str], imports: &HashMap<&str, HashSet<&'a str>>) -> Vec<&'a str> {
    let mut sorted = paths.to_vec();
    sorted.sort_by(|a, b| locale_cmp(a, b));
    let mut remaining: HashSet<&str> = paths.iter().copied().collect();
    let mut ordered = Vec::with_capacity(paths.len());
    while !remaining.is_empty() {
        let ready = |p: &&str| imports.get(*p).is_none_or(|deps| deps.iter().all(|d| !remaining.contains(d)));
        let next = sorted.iter().copied().filter(|p| remaining.contains(p)).find(ready).or_else(|| sorted.iter().copied().find(|p| remaining.contains(p)));
        let Some(next) = next else { break };
        remaining.remove(next);
        ordered.push(next);
    }
    ordered
}

/// testedSourcePosition: `a.logic.test.ts` covers `a.logic.ts` while `a.browser.test.ts` covers
/// `a.ts`, so the longest dotted prefix that names a source file wins.
fn tested_source_position(path: &str, positions: &HashMap<&str, usize>) -> usize {
    let name = path.rsplit('/').next().unwrap_or(path);
    let mut name = [".test.", ".spec."].iter().filter_map(|t| name.find(*t)).min().map_or(name, |cut| &name[..cut]);
    loop {
        if let Some(&position) = positions.get(name) {
            return position;
        }
        match name.rfind('.') {
            Some(dot) if dot > 0 => name = &name[..dot],
            _ => return usize::MAX,
        }
    }
}

/// orderDiffFiles: sources in import order, then tests by the source they cover, then generated
/// files. Gives indices into `files`, each path once (its last file).
fn order_diff_files(files: &[(&str, Vec<&str>)]) -> Vec<usize> {
    let mut paths: Vec<&str> = vec![];
    let mut by_path: HashMap<&str, usize> = HashMap::new();
    for (n, (path, _)) in files.iter().enumerate() {
        if by_path.insert(*path, n).is_none() {
            paths.push(*path);
        }
    }
    let of = |wanted: Tier| paths.iter().copied().filter(|p| tier(p) == wanted).collect::<Vec<_>>();
    let sources = of(Tier::Source);
    let mut by_module: HashMap<&str, &str> = HashMap::new();
    let mut by_base: HashMap<&str, &str> = HashMap::new();
    let mut ambiguous: HashSet<&str> = HashSet::new();
    for &path in &sources {
        by_module.insert(strip_extension(path), path);
        let base = base_name(path);
        if by_base.insert(base, path).is_some() {
            ambiguous.insert(base);
        }
    }
    for base in ambiguous {
        by_base.remove(base);
    }
    let imports: HashMap<&str, HashSet<&str>> = sources.iter().map(|path| (*path, imported_paths(path, &files[by_path[path]].1, &by_module, &by_base))).collect();
    let ordered = order_by_imports(&sources, &imports);

    let mut positions: HashMap<&str, usize> = HashMap::new();
    for (n, path) in ordered.iter().enumerate() {
        positions.entry(base_name(path)).or_insert(n);
    }
    let mut tests = of(Tier::Test);
    tests.sort_by(|a, b| tested_source_position(a, &positions).cmp(&tested_source_position(b, &positions)).then_with(|| locale_cmp(a, b)));
    let mut generated = of(Tier::Generated);
    generated.sort_by(|a, b| locale_cmp(a, b));
    ordered.into_iter().chain(tests).chain(generated).map(|path| by_path[path]).collect()
}

/// orderDiffFilesLikeTree: each folder kept together and ranked where its first file is, so the
/// list never disagrees with the tree. Gives indices into `paths`.
fn tree_order(paths: &[&str]) -> Vec<usize> {
    let mut positions: HashMap<&str, usize> = HashMap::new();
    for (n, &path) in paths.iter().enumerate() {
        positions.insert(path, n);
        for (slash, _) in path.match_indices('/') {
            positions.entry(&path[..=slash]).or_insert(n);
        }
    }
    let key = |path: &str| path.match_indices('/').map(|(slash, _)| positions[&path[..=slash]]).chain([positions[path]]).collect::<Vec<_>>();
    let mut order: Vec<usize> = (0..paths.len()).collect();
    order.sort_by_cached_key(|&n| key(paths[n]));
    order
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
        c.reveal = None;
        c.replying = None;
        c.next = None;
        c.pages.clear();
        c.paging = None;
        c.page_failed = false;
        c.loaded.clear();
        c.editing = None;
        c.full_comments.clear();
        c.raw.clear();
        c.omitted.clear();
        changed = true;
    }
    changed |= sync_refetch(s, &entry);
    let c = &mut s.ui.pages.code;
    c.diff.set_mode(mode);
    c.diff.set_wrap(wrap);
    if std::mem::take(&mut c.fresh) {
        // A new first page: pages read for the old one target another diff.
        c.pages.clear();
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
            s.ui.pages.code.reveal = Some(path);
            changed = true;
        }
        changed |= sync_page(s, &entry) | sync_notes(s);
    }
    changed |= sync_thread_command(s, &entry) | sync_comment_pages(s, &entry) | diff_find::refresh(s, Host::PullRequest);
    sync_viewed(s, &entry) | changed
}

/// answerReveal: a file asked for from outside the tab (a guide's link) is scrolled to once it
/// is loaded, the diff paging forward until it arrives.
pub fn answer_reveal(s: &mut App, tx: &mut Tx) -> bool {
    let Some(path) = s.ui.pages.code.reveal.clone() else { return false };
    if s.ui.pages.detail.files.is_none() {
        return false;
    }
    match s.ui.pages.code.diff.file_index(&path) {
        Some(f) => {
            let c = &mut s.ui.pages.code;
            c.reveal = None;
            c.diff.scroll_to_file(f, tx);
            true
        }
        None => load_more(s),
    }
}

/// The diff view and its scroller, a direct child: `SCROLLER` is the key path Rune resolves
/// through the nearest keyed ancestor, for layout reads and scroll requests alike.
const VIEW_KEY: &str = "pr-code-view";
const SCROLL_KEY: &str = "scroll";
pub(super) const SCROLLER: &str = "pr-code-view/scroll";

/// backgroundRefreshToken: a finished turn reads the diff's first page and the viewed ticks
/// again, keeping the pages read so far while the first is unchanged.
pub(super) fn turn_finished(s: &mut App) {
    s.ui.pages.code.viewed.refresh = true;
    if s.ui.pages.detail.files.is_none() || s.ui.pages.code.refetch.is_some() {
        return;
    }
    let Some(entry) = s.ui.pages.detail.entry.clone() else { return };
    let mut input = reference(&entry);
    if let Some(oid) = scoped_commit(&s.ui.pages.detail) {
        input["commit"] = json!(oid);
    }
    let scope = s.ui.pages.code.scope.clone();
    let Some(net) = super::pull_requests::net_for(s, &entry) else { return };
    let id = net.http_request("POST", "/api/pull-requests/diff", input);
    s.ui.pages.code.refetch = Some((id, scope));
}

/// The reread first page replaces the diff, as a newly read one does; a failed one leaves it.
fn sync_refetch(s: &mut App, entry: &Value) -> bool {
    let Some((id, asked)) = s.ui.pages.code.refetch.clone() else { return false };
    let Some(got) = reply(s, entry, id) else { return false };
    s.ui.pages.code.refetch = None;
    if asked != s.ui.pages.code.scope {
        return true;
    }
    if let Ok(d) = got {
        merge_first_page(s, &d);
    }
    true
}

/// What decides whether a reread page changed (the slice comparison): its patch, truncation,
/// next cursor and omitted file stats.
pub(super) fn page_mark(d: &Value) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for key in ["patch", "truncated", "nextCursor", "omittedFileStats"] {
        match &d[key] {
            Value::String(text) => text.hash(&mut h),
            other => other.to_string().hash(&mut h),
        }
    }
    h.finish()
}

/// The page merge: an unchanged first page keeps the diff and every page read after it, so the
/// scroll stays; a changed one replaces them, since later cursors target the old diff.
fn merge_first_page(s: &mut App, d: &Value) -> bool {
    if s.ui.pages.code.first_mark == Some(page_mark(d)) {
        return false;
    }
    first_page(s, d);
    true
}

/// A first page read again replaces the diff, as a newly read one does.
fn first_page(s: &mut App, d: &Value) {
    let patch = d["patch"].as_str().unwrap_or("");
    let files = rune_kit::diff::parse(patch);
    s.ui.pages.code.raw = unparsed(patch, &files);
    let u = &mut s.ui.pages.detail;
    u.files = Some(files);
    u.diff_truncated = d["truncated"].as_bool().unwrap_or(false);
    let c = &mut s.ui.pages.code;
    c.next = d["nextCursor"].as_str().map(str::to_owned);
    c.first_mark = Some(page_mark(d));
    c.omitted.clear();
    absorb_omitted(c, d);
    c.fresh = true;
}

/// A page's omittedFileStats, kept beside the pages read before it.
pub(super) fn absorb_omitted(c: &mut Ui, d: &Value) {
    for f in d["omittedFileStats"].as_array().into_iter().flatten() {
        let n = |key: &str| f[key].as_u64().unwrap_or(0) as usize;
        c.omitted.insert(field(f, "path").to_owned(), (n("additions"), n("deletions")));
    }
}

/// A file header's counts: what its patch shows, or for a file the host withheld (none shown),
/// the counts the host reported for it.
fn header_counts(c: &Ui, file: &FileDiff) -> (usize, usize) {
    let path = file.path();
    match c.counts.get(path).copied().unwrap_or_else(|| file_counts(file)) {
        (0, 0) => c.omitted.get(path).copied().unwrap_or((0, 0)),
        shown => shown,
    }
}

/// The page's patch when the parser read no file from it.
pub(super) fn unparsed(patch: &str, files: &[FileDiff]) -> Vec<String> {
    if files.is_empty() && !patch.trim().is_empty() { vec![patch.to_owned()] } else { vec![] }
}

fn show(s: &mut App) {
    let (default, ws, hide) = (fold_default(s), ignore_ws(s), hide_tests(s));
    let u = &s.ui.pages.detail;
    let source = u.files.as_deref().unwrap_or(&[]);
    let files = shown_files(ordered_files(source, &s.ui.pages.code.pages), hide, ws, s.ui.pages.code.revealed.as_deref());
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
            let patch = d["patch"].as_str().unwrap_or("");
            let more = rune_kit::diff::parse(patch);
            let raw = unparsed(patch, &more);
            s.ui.pages.code.raw.extend(raw);
            s.ui.pages.code.pages.push(more.len());
            absorb_omitted(&mut s.ui.pages.code, &d);
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
        if let Some(prev) = old.entry.filter(|_| !old.queued.is_empty()) {
            let mut batch: Vec<(String, bool)> = old.queued.into_iter().collect();
            batch.sort();
            if let Some(id) = super::pull_requests::request_for(s, &prev, "pullRequests.setFilesViewed", set_viewed_input(&prev, &batch)) {
                leaving.push((prev, id));
            }
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
    let read_id = if read { super::pull_requests::request_for(s, entry, "pullRequests.filesViewed", reference(entry)) } else { None };
    let write_id = batch.as_ref().and_then(|b| super::pull_requests::request_for(s, entry, "pullRequests.setFilesViewed", set_viewed_input(entry, b)));
    let v = &mut s.ui.pages.code.viewed;
    if let Some(id) = read_id {
        v.reading = Some((id, v.reads));
        v.reads += 1;
        v.refresh = false;
    }
    if let (Some(id), Some(b)) = (write_id, batch) {
        v.sending = Some((id, b));
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

/// "Add to agent" (addSelectionToAgent through startAsk): a new thread holding the request, the
/// pull request and the lines. Web attaches to a thread's composer only where the detail sits in
/// that thread's right panel, which this page never does.
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
    super::pr_detail::ask(s, super::pr_detail::bounded(&body), vec![chip, comment]);
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
    match super::pull_requests::request_for(s, &entry, method, input) {
        Some(id) => {
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
            // A reply returns no comment, so a thread read past its first page reads on again.
            if failure == REPLY_FAILED
                && let Some((thread, _)) = s.ui.pages.code.replying.take()
            {
                let cursor = threads(s).iter().find(|th| field(th, "id") == thread).and_then(|th| th["nextCommentsCursor"].as_str()).map(str::to_owned);
                if let Some(page) = s.ui.pages.code.loaded.get_mut(&thread) {
                    page.1 = page.1.take().or(cursor);
                }
            }
            if failure == EDIT_FAILED
                && let Some((id, draft)) = s.ui.pages.code.editing.take()
            {
                edit_loaded(&mut s.ui.pages.code.loaded, &id, &draft.edit.text);
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

/// loadMore: the thread's next page of comments, one read per thread at a time.
fn load_comments(s: &mut App, thread: &str, cursor: &str) {
    if s.ui.pages.code.comment_pages.iter().any(|(_, on, _)| on == thread) {
        return;
    }
    let Some(entry) = s.ui.pages.detail.entry.clone() else { return };
    let mut input = reference(&entry);
    input["threadId"] = json!(thread);
    input["cursor"] = json!(cursor);
    let scope = s.ui.pages.code.scope.clone();
    match super::pull_requests::request_for(s, &entry, "pullRequests.threadComments", input) {
        Some(id) => {
            s.ui.pages.code.comment_pages.push((id, thread.to_owned(), scope));
        }
        None => crate::shell::palette::toast_error(s, COMMENTS_FAILED, None),
    }
}

fn sync_comment_pages(s: &mut App, entry: &Value) -> bool {
    let mut changed = false;
    for (id, thread, asked) in std::mem::take(&mut s.ui.pages.code.comment_pages) {
        let Some(got) = reply(s, entry, id) else {
            s.ui.pages.code.comment_pages.push((id, thread, asked));
            continue;
        };
        changed = true;
        if asked != s.ui.pages.code.scope {
            continue;
        }
        match got {
            Ok(page) => take_page(&mut s.ui.pages.code.loaded, &thread, &page),
            Err(_) => crate::shell::palette::toast_error(s, COMMENTS_FAILED, None),
        }
    }
    changed
}

/// A page of a thread's comments joins the ones read before it.
fn take_page(loaded: &mut HashMap<String, (Vec<Value>, Option<String>)>, thread: &str, page: &Value) {
    let had = loaded.remove(thread).map(|l| l.0).unwrap_or_default();
    let comments = merge_comments(&had, list(Some(page), "comments"));
    loaded.insert(thread.to_owned(), (comments, page["nextCursor"].as_str().map(str::to_owned)));
}

/// mergePullRequestThreadComments: `more` after `base`, each comment once.
fn merge_comments(base: &[Value], more: &[Value]) -> Vec<Value> {
    let mut seen: HashSet<String> = base.iter().map(|c| field(c, "id").to_owned()).collect();
    let mut out = base.to_vec();
    out.extend(more.iter().filter(|c| seen.insert(field(c, "id").to_owned())).cloned());
    out
}

/// editPullRequestThreadComment over every thread's read pages.
fn edit_loaded(loaded: &mut HashMap<String, (Vec<Value>, Option<String>)>, id: &str, body: &str) {
    for page in loaded.values_mut() {
        for c in &mut page.0 {
            if field(c, "id") == id {
                c["body"] = json!(body);
            }
        }
    }
}

/// A comment some thread read past its first page.
pub(super) fn loaded_comment<'a>(s: &'a App, id: &str) -> Option<&'a Value> {
    s.ui.pages.code.loaded.values().flat_map(|page| &page.0).find(|c| field(c, "id") == id)
}

/// canEditPullRequestComment for a review thread's comment.
fn can_edit(s: &App, comment: &Value) -> bool {
    let probe = json!({"kind": "review-comment", "author": comment["author"], "canEdit": comment["canEdit"]});
    s.ui.pages.detail.detail.as_ref().is_some_and(|d| super::pr_edit::can_edit_comment(d, &probe))
}

fn saving_edit(c: &Ui) -> bool {
    c.thread_pending.is_some_and(|(_, failure)| failure == EDIT_FAILED)
}

fn start_edit(s: &mut App, id: &str, body: &str) {
    let c = &mut s.ui.pages.code;
    if !saving_edit(c) {
        c.editing = Some((id.to_owned(), EditDraft::new(body)));
    }
}

/// The editor's onSave: a comment may not be emptied.
fn save_edit(s: &mut App) {
    let Some((id, draft)) = &s.ui.pages.code.editing else { return };
    if draft.edit.text.trim().is_empty() {
        return;
    }
    let fields = json!({"commentId": id, "kind": "review-comment", "body": draft.edit.text});
    thread_command(s, "pullRequests.updateComment", fields, EDIT_FAILED);
}

fn edit_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if super::pr_edit::submits(e) {
        if !tx.key_repeat() {
            save_edit(s);
        }
        return true;
    }
    if e.key != Key::Escape {
        return false;
    }
    s.ui.pages.code.editing = None;
    true
}

/// startFixFinding for a review thread: the project's new-thread draft holding the task, with the
/// thread attached on its line.
fn fix_thread(s: &mut App, th: &Value) {
    let Some(d) = s.ui.pages.detail.detail.clone() else { return };
    let (text, records) = super::pr_detail::finding_handoff(&d, &super::pr_detail::Finding::Thread(th.clone()));
    super::pr_detail::handoff(s, text, records.into_iter().next());
}

/// Whether the toolbar is under `WIDE_TOOLBAR`, as web's `@lg` container query reads it: its own
/// width last frame, else the detail's that it spans, else the last answer, so a tab switch or a
/// reopened panel never paints the wide toolbar for a frame.
fn narrow_toolbar(toolbar: Option<bool>, panel: impl FnOnce() -> Option<bool>, last: Option<bool>) -> bool {
    toolbar.or_else(panel).or(last).unwrap_or(false)
}

/// FileExplorerPane's first width: 320, or two fifths of the row when that is less, at least 192.
fn tree_default(row: f32) -> f32 {
    (row * 0.4).min(320.0).max(192.0)
}

/// FileExplorerPane's bounds: 192 up to 560, leaving the diff 256 unless the first width needs it.
fn tree_width(w: f32, row: f32) -> f32 {
    let max = (row - 256.0).max(tree_default(row)).min(560.0).max(192.0);
    w.clamp(192.0, max)
}

/// The resize handle's keys: Left widens and Right narrows by 16, Home and End go to the bounds.
fn resize_step(key: &Key, w: f32, row: f32) -> Option<f32> {
    let next = match key {
        Key::Left => w + 16.0,
        Key::Right => w - 16.0,
        Key::Home => 0.0,
        Key::End => f32::MAX,
        _ => return None,
    };
    Some(tree_width(next, row))
}

/// A folder row's keys: Right opens it, Left folds it.
fn fold_key(folded: &mut HashSet<String>, dir: &str, key: &Key) -> bool {
    match key {
        Key::Right => folded.remove(dir),
        Key::Left if !folded.contains(dir) => folded.insert(dir.to_owned()),
        _ => false,
    }
}

/// The tree's width: the one being dragged, else the saved one, else the first.
fn tree_shown(s: &App, row: f32) -> f32 {
    let saved = s.client_settings.values.get(TREE_WIDTH_KEY).and_then(Value::as_f64).map(|w| w as f32);
    s.ui.pages.code.tree_drag.or(saved).map_or_else(|| tree_default(row), |w| tree_width(w, row))
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
pub(crate) fn unfold(s: &mut App, path: &str) -> Option<usize> {
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
    crate::panel::diff::set_hide_test_files(s, hide);
}

pub(crate) fn refresh_test_filter(s: &mut App) {
    if s.ui.pages.detail.files.is_some() {
        show(s);
    }
}

#[cfg(test)]
pub(crate) fn test_file_shown(s: &App) -> bool {
    s.ui.pages.code.diff.files().iter().any(|f| crate::panel::diff::is_test_file(f.path()))
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
    if u.diff_truncated || !c.raw.is_empty() {
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
    let toolbar = rune::memo::layout_with("pr-code-toolbar", |r| r[2] < WIDE_TOOLBAR);
    let narrow = narrow_toolbar(toolbar, || rune::memo::layout_with("pr-detail", |r| r[2] < WIDE_TOOLBAR), c.narrow.get());
    c.narrow.set(Some(narrow));
    // The thread cards' reaction bars sit in diff rows, which hold no overlay layer.
    super::pr_edit::reaction_picker(s, t, overlays);
    if narrow {
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
    let c = &s.ui.pages.code;
    // Raw pages show once no page is left, as the sentinel still pages until then.
    let raw = (c.next.is_none() && !c.raw.is_empty()).then(|| raw_pages(t, &c.raw));
    if c.diff.files().is_empty()
        && let Some(raw) = raw
    {
        return raw;
    }
    let mut main = row().key("pr-code-main").grow(1.0).min_h(0.0).w_full().items_stretch().child(diff_view(s, t));
    if tree_open(s) && !s.ui.pages.code.diff.files().is_empty() {
        let row_w = rune::memo::layout_with("pr-code-main", |r| r[2]);
        if let Some(w) = row_w {
            s.ui.pages.code.tree_row.set(w);
        }
        main = main.child(tree_handle(t)).child(tree(s, t, row_w.map(|w| tree_shown(s, w))));
    }
    col().grow(1.0).min_h(0.0).w_full().children(orphans(s, t)).child(main).children(raw.map(|r| col().w_full().max_h(256.0).min_h(0.0).child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.6))).child(r)))
}

/// The pages the parser could not read, as text (rawSlicesView and unstructured).
fn raw_pages(t: &T, raw: &[String]) -> El {
    let mut list = col().w_full().gap(16.0).px(16.0).py(20.0);
    for patch in raw {
        list = list.child(
            col()
                .w_full()
                .gap(8.0)
                .child(text(RAW_REASON).font(theme::TEXT_XS).color(t.c.muted_foreground))
                .child(text(patch.clone()).font(theme::mono(theme::TEXT_XS)).color(t.c.foreground).wrap().selectable(true)),
        );
    }
    col().key("pr-code-raw").grow(1.0).min_h(0.0).w_full().child(list).scroll(ScrollSpec { stick_bottom: false })
}

/// FileExplorerPane's resize handle: the tree's left edge, so moving left widens it; the width is
/// saved once the drag ends.
fn tree_handle(t: &T) -> El {
    el().key("pr-code-tree-resize")
        .w(1.0)
        .self_stretch()
        .fill(t.c.border.alpha_mul(0.6))
        .cursor(Cursor::ResizeColumn)
        .role("separator")
        .label("Resize PR Code files")
        .focusable(true)
        .on_key(|s: &mut App, e: &KeyEvent, _: &mut Tx| {
            if e.mods.alt || e.mods.ctrl || e.mods.cmd || e.mods.logo {
                return false;
            }
            let row = s.ui.pages.code.tree_row.get();
            let Some(w) = resize_step(&e.key, tree_shown(s, row), row) else { return false };
            s.client_settings.values.insert(TREE_WIDTH_KEY.into(), json!(w.round()));
            s.client_settings.save();
            true
        })
        .on_drag(|s: &mut App, d: &mut rune::Drag, _: &mut Tx| {
            let row = s.ui.pages.code.tree_row.get();
            if d.cancelled() {
                s.ui.pages.code.tree_drag = None;
                return;
            }
            match d.phase {
                rune::Phase::Down => d.data = tree_shown(s, row),
                rune::Phase::Move => s.ui.pages.code.tree_drag = Some(tree_width(d.data - d.dx, row)),
                rune::Phase::Up => {
                    if let Some(w) = s.ui.pages.code.tree_drag.take() {
                        s.client_settings.values.insert(TREE_WIDTH_KEY.into(), json!(w.round()));
                        s.client_settings.save();
                    }
                }
            }
        })
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
    let page = c.loaded.get(&id);
    let comments = merge_comments(list(Some(th), "comments"), page.map_or(&[][..], |p| p.0.as_slice()));
    let next = match page {
        Some(p) => p.1.clone(),
        None => th["nextCommentsCursor"].as_str().map(str::to_owned),
    };
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
    let fixed = th.clone();
    let fix = kit::Button::new("Fix in a thread").icon(&i::HAMMER).variant("ghost").size("xs").disabled(c.thread_pending.is_some()).on_press(move |s: &mut App| fix_thread(s, &fixed));
    head = head.child(fix.build(t));
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
    for comment in &comments {
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
        meta = meta.child(spacer()).children(super::pr_edit::reaction_bar(s, t, comment));
        let cid = field(comment, "id").to_owned();
        let body = match &c.editing {
            Some((on, draft)) if *on == cid => comment_editor(s, t, draft),
            _ => {
                let mut line = row().w_full().gap(4.0).items_start().child(super::pr_media::view(s, t, field(comment, "body")).grow(1.0).shrink().min_w(0.0));
                if can_edit(s, comment) {
                    let shown = field(comment, "body").to_owned();
                    line = line.child(super::pr_edit::pencil(t, "Edit comment", move |s: &mut App| start_edit(s, &cid, &shown)));
                }
                line
            }
        };
        thread = thread.child(col().w_full().min_w(0.0).gap(4.0).hover_group().child(meta).child(body));
    }
    card = card.child(thread);
    if let Some(cursor) = next {
        let loading = c.comment_pages.iter().any(|(_, on, _)| *on == id);
        let more = id.clone();
        let label = if loading { "Loading..." } else { "Load more comments" };
        card = card.child(row().mt(8.0).child(kit::Button::new(label).variant("ghost").size("xs").disabled(loading).on_press(move |s: &mut App| load_comments(s, &more, &cursor)).build(t)));
    }
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

/// PullRequestMarkdownEditor on a thread comment: Write or Preview, Cancel and Save; Cmd/Ctrl+Enter
/// saves, Escape cancels.
fn comment_editor(s: &App, t: &T, draft: &EditDraft) -> El {
    let saving = saving_edit(&s.ui.pages.code);
    let empty = draft.edit.text.trim().is_empty();
    let mode = kit::ToggleGroup::new(&super::pr_edit::EDITOR_MODES, &[usize::from(draft.preview)])
        .on_pick(|s: &mut App, n: usize, _: &mut Tx| {
            let c = &mut s.ui.pages.code;
            if !saving_edit(c)
                && let Some((_, d)) = &mut c.editing
            {
                d.preview = n == 1;
            }
        })
        .build(&kit::theme(t))
        .label("Markdown editor mode");
    let body = if draft.preview {
        let shown = if empty { text("Nothing to preview.").font(theme::TEXT_XS).color(t.c.muted_foreground) } else { super::pr_media::view(s, t, &draft.edit.text) };
        col().w_full().px(12.0).py(8.0).radius(radius::LG).border(1.0, t.c.border.alpha_mul(0.6)).child(shown)
    } else {
        let input = super::pr_edit::area(t, "pr-code-edit-comment".into(), &draft.edit, 6.0, "Edit comment".into(), "");
        if saving {
            input.disabled(true).opacity(0.64)
        } else {
            input.on_edit(|s: &mut App| &mut s.ui.pages.code.editing.get_or_insert_with(Default::default).1.edit).on_input_key(edit_key)
        }
    };
    let buttons = row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(kit::Button::new("Cancel").variant("ghost").size("xs").disabled(saving).on_press(|s: &mut App| s.ui.pages.code.editing = None).build(t))
        .child(kit::Button::new(if saving { "Saving..." } else { "Save" }).variant("outline").size("xs").disabled(!writable(s) || saving || empty).on_press(save_edit).build(t));
    col().key("pr-code-comment-editor").w_full().mt(4.0).gap(8.0).child(row().child(mode)).child(body).child(buttons)
}

/// PullRequestCommentBody: a comment taller than 240 shows its top, fading out, until "Show full
/// comment". For the Summary's comment cards (pr_detail), keyed by the comment's id.
pub(super) fn clipped_comment(s: &App, t: &T, id: &str, body: El) -> El {
    let key = format!("pr-comment-clip-{id}");
    let overflowing = rune::memo::layout_with(&format!("{key}/body"), |r| r[3] > COMMENT_CLIP).unwrap_or(false);
    let expanded = s.ui.pages.code.full_comments.contains(id);
    let mut clip = col().key(key).w_full().min_w(0.0).child(col().key("body").w_full().min_w(0.0).child(body));
    if !expanded {
        clip = clip.max_h(COMMENT_CLIP).clip_fade(0.0, 0.0, 0.0, if overflowing { 40.0 } else { 0.0 });
    }
    let mut out = col().w_full().min_w(0.0).child(clip);
    if overflowing {
        let toggled = id.to_owned();
        let button = kit::Button::new(if expanded { "Show less" } else { "Show full comment" })
            .variant("ghost-muted")
            .size("xs")
            .on_press(move |s: &mut App| {
                let full = &mut s.ui.pages.code.full_comments;
                if !full.remove(&toggled) {
                    full.insert(toggled.clone());
                }
            })
            .build(t)
            .expanded(expanded);
        out = out.child(row().mt(8.0).child(button));
    }
    out
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
    if !wrap && let Some(r) = rune::memo::layout(SCROLLER) {
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
    let scroll = col().key(SCROLL_KEY).w_full().grow(1.0).min_h(0.0).scroll(ScrollSpec { stick_bottom: false }).on_scroll_offset(super::pr_detail::condense_reporter(s, 2));
    let scroll = if wrap { scroll } else { scroll.scroll_xy() };
    let footer = s.ui.pages.code.next.is_some().then(|| page_footer(s, t));
    let mut root = col().key(VIEW_KEY).focusable(true).grow(1.0).min_w(0.0).min_h(0.0).self_stretch().fill(look.bg).child(scroll.child(list).children(footer));
    if let Some(f) = st.sticky() {
        root = root.child(file_header(s, &look, f, true).key("sticky").absolute(0.0, 0.0).w_full());
    }
    if let Some(bar) = diff_find::bar(s, &look.t, Host::PullRequest) {
        root = root.child(bar.pin(None, Some(HEADER_H + 8.0), Some(12.0), None));
    }
    root.on_key(|s: &mut App, e, tx| diff_find::key(s, Host::PullRequest, e, tx) || s.ui.pages.code.diff.key(e, tx))
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
    let (adds, dels) = header_counts(&s.ui.pages.code, file);
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

/// DiffFileTree: the files by folder; pressing one opens it and scrolls to it. `width` waits on
/// the row's first layout.
/// DiffFileTree's per-type icon (T3_PIERRE_ICONS), the layers the diff panel draws.
fn file_mark(t: &T, path: &str) -> El {
    crate::panel::diff::file_icons::for_path(path).iter().fold(el().size(16.0, 16.0), |mark, &(glyph, light, dark)| {
        mark.child(icon(glyph, 16.0).color(rune::color::hex(if t.dark { dark } else { light }).unwrap_or(t.c.muted_foreground)).absolute(0.0, 0.0))
    })
}

fn tree(s: &App, t: &T, width: Option<f32>) -> El {
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
    let mut list = col().key("pr-code-tree-rows").w_full().padding(4.0).gap(1.0).role("tree").label(name).focus_group();
    for (n, r) in rows.into_iter().enumerate() {
        let pad = 8.0 + r.depth as f32 * 12.0;
        let base = row().key(format!("t{n}")).h(24.0).pl(pad).pr(8.0).gap(6.0).radius(6.0).center_y().w_full().cursor(Cursor::Pointer).focusable(true).role("treeitem").level(r.depth + 1);
        let line = if let Some(dir) = r.dir {
            let open = !c.folded_dirs.contains(&dir);
            let keyed = dir.clone();
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
                .on_key(move |s: &mut App, e: &KeyEvent, _: &mut Tx| fold_key(&mut s.ui.pages.code.folded_dirs, &keyed, &e.key))
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
                .focus_selected(selected)
                .fill(if selected { t.c.foreground.alpha(0.12) } else { Color::TRANSPARENT })
                .hover(t.c.foreground.alpha(if selected { 0.12 } else { 0.07 }))
                .label(path.clone())
                .child(file_mark(t, &path))
                .child(text(r.label).font(theme::TEXT_XS).color(color).single_line().truncate().min_w(0.0).shrink())
                .on_press_tx(move |s: &mut App, tx: &mut Tx| reveal(s, &path, tx))
        };
        list = list.child(line);
    }
    let out = col().key("pr-code-tree");
    let out = match width {
        Some(w) => out.w(w),
        None => out.w_frac(0.4).max_w(320.0),
    };
    let out = out
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
/// `threads` (a conversation inline, and two no line holds, opened), `thread-edit` and
/// `thread-reactions` (its comments reacted to, one more page read, and one comment being edited
/// or the reaction picker open), `options` (the narrow
/// toolbar's menu open), `find` (find in the diff over three matches), `raw` (a page the parser
/// could not read), `tree-resized` (the file tree at a saved 420), `more-files` (a next
/// page to read), `file-order` (sources by import, then tests, then generated files, as the tree
/// lists them), `pending-review` (two comments
/// pending, for the composer) and `line-comment` (a pending comment and a ranged draft).
/// A binary file, a file too large to inline, a pure rename and a copy with a change.
const WITHHELD_PATCH: &str = "diff --git a/assets/icon.png b/assets/icon.png\nBinary files a/assets/icon.png and b/assets/icon.png differ\n\
diff --git a/data/fixtures/large.json b/data/fixtures/large.json\nindex 1111111..2222222 100644\n\
diff --git a/src/old_name.rs b/src/new_name.rs\nsimilarity index 100%\nrename from src/old_name.rs\nrename to src/new_name.rs\n\
diff --git a/src/base.rs b/src/copy.rs\nsimilarity index 90%\ncopy from src/base.rs\ncopy to src/copy.rs\n--- a/src/base.rs\n+++ b/src/copy.rs\n@@ -1 +1 @@\n-old\n+new\n";

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
        "thread-edit" | "thread-reactions" => {
            scene(s, "threads");
            if let Some(d) = &mut s.ui.pages.detail.detail {
                d["capabilities"]["reactions"] = json!(true);
                d["capabilities"]["edit"]["comment"] = json!(true);
            }
            if let Some(th) = s.ui.pages.detail.activity.as_mut().and_then(|a| a["reviewThreads"].get_mut(0)) {
                th["commentCount"] = json!(4);
                if let Some(first) = th["comments"].get_mut(0) {
                    first["reactions"] = json!([{"content": "thumbs-up", "count": 2, "actors": ["theo"], "viewerHasReacted": true}, {"content": "eyes", "count": 1, "actors": ["theo"], "viewerHasReacted": false}]);
                }
                if let Some(second) = th["comments"].get_mut(1) {
                    second["canEdit"] = json!(true);
                }
            }
            let later = json!({"id": "tc5", "author": {"login": "theo", "name": "Theo", "avatarUrl": null}, "body": "The zone is read once per render now, fine by me.", "createdAt": "2026-11-06T10:02:00.000Z", "url": null});
            s.ui.pages.code.loaded.insert("t1".into(), (vec![later], Some("c3".into())));
            if kind == "thread-edit" {
                s.ui.pages.code.editing = Some(("tc2".into(), EditDraft::new("Yes, switched to the local IANA zone, read once per render.")));
            } else {
                s.ui.pages.detail.edit.picker = Some("tc1".into());
            }
        }
        "options" => {
            unfold(s, usage);
            s.ui.pages.code.options = true;
        }
        "find" => {
            unfold(s, usage);
            s.ui.pages.code.find = Some(Find::default());
            s.ui.pages.code.find_edit = TextEdit::new("wall");
            diff_find::refresh(s, Host::PullRequest);
        }
        "withheld" => {
            s.ui.pages.detail.files = Some(rune_kit::diff::parse(WITHHELD_PATCH));
            s.ui.pages.detail.diff_truncated = true;
            s.ui.pages.code.pages.clear();
            absorb_omitted(&mut s.ui.pages.code, &json!({"omittedFileStats": [{"path": "data/fixtures/large.json", "additions": 4210, "deletions": 96}]}));
            show(s);
        }
        "raw" => {
            s.ui.pages.detail.files = Some(vec![]);
            s.ui.pages.code.raw = vec!["Binary files a/apps/desktop-rune/assets/icon.png and b/apps/desktop-rune/assets/icon.png differ\n".into()];
            show(s);
        }
        "file-order" => {
            s.client_settings.values.insert(TREE_KEY.into(), json!(true));
            let patch: String = [
                ("pnpm-lock.yaml", "lockfileVersion: '9.0'"),
                ("apps/web/src/components/Button.test.tsx", "import { Button } from \"./Button\";"),
                ("apps/web/src/components/Button.tsx", "import { tone } from \"../lib/theme\";"),
                ("apps/web/dist/app.js", "export {};"),
                ("apps/web/src/components/Panel.tsx", "import { Button } from \"./Button\";"),
                ("apps/web/src/lib/theme.ts", "export const tone = \"muted\";"),
            ]
            .iter()
            .map(|(path, line)| format!("diff --git a/{path} b/{path}\nnew file mode 100644\n--- /dev/null\n+++ b/{path}\n@@ -0,0 +1 @@\n+{line}\n"))
            .collect();
            s.ui.pages.detail.files = Some(rune_kit::diff::parse(&patch));
            s.ui.pages.code.pages.clear();
            show(s);
        }
        "tests-hidden" => {
            s.client_settings.values.insert(HIDE_TESTS_KEY.into(), json!(true));
            let path = "apps/desktop-rune/src/pages/usage.test.rs";
            s.ui.pages.detail.files = Some(rune_kit::diff::parse(&format!("diff --git a/{path} b/{path}\n--- a/{path}\n+++ b/{path}\n@@ -1 +1 @@\n-x\n+y\n")));
            s.ui.pages.code.pages.clear();
            show(s);
        }
        "tree-resized" => {
            s.client_settings.values.insert(TREE_KEY.into(), json!(true));
            s.client_settings.values.insert(TREE_WIDTH_KEY.into(), json!(420));
            unfold(s, usage);
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
    fn the_narrow_toolbar_reads_the_last_frame_before_painting_wide() {
        assert!(narrow_toolbar(Some(true), || Some(false), Some(false)), "its own width wins");
        assert!(!narrow_toolbar(Some(false), || Some(true), Some(true)));
        assert!(narrow_toolbar(None, || Some(true), Some(false)), "a tab switch reads the panel it spans");
        assert!(narrow_toolbar(None, || None, Some(true)), "a reopened panel keeps the last answer");
        assert!(!narrow_toolbar(None, || None, None), "nothing measured yet reads wide");
    }

    #[test]
    fn the_file_tree_starts_at_320_or_two_fifths_and_resizes_within_its_bounds() {
        assert_eq!((tree_default(1000.0), tree_default(600.0), tree_default(300.0)), (320.0, 240.0, 192.0));
        assert_eq!((tree_width(100.0, 1000.0), tree_width(900.0, 1000.0)), (192.0, 560.0));
        assert_eq!(tree_width(300.0, 500.0), 244.0, "the diff keeps 256");
        assert_eq!(tree_width(300.0, 300.0), 192.0, "never under the first width");
        let mut s = App::new();
        s.client_settings.values.remove(TREE_WIDTH_KEY);
        assert_eq!(tree_shown(&s, 1000.0), 320.0);
        s.client_settings.values.insert(TREE_WIDTH_KEY.into(), json!(900));
        assert_eq!(tree_shown(&s, 1000.0), 560.0, "a saved width is clamped to the row");
        s.ui.pages.code.tree_drag = Some(250.0);
        assert_eq!(tree_shown(&s, 1000.0), 250.0, "the drag wins over the saved width");
    }

    #[test]
    fn a_threads_later_comments_join_once_and_an_edit_reaches_them() {
        let c = |id: &str, body: &str| json!({"id": id, "body": body});
        let mut loaded = HashMap::new();
        take_page(&mut loaded, "t", &json!({"comments": [c("a", "1"), c("b", "2")], "nextCursor": "n2"}));
        take_page(&mut loaded, "t", &json!({"comments": [c("b", "2"), c("c", "3")], "nextCursor": null}));
        assert_eq!(loaded["t"], (vec![c("a", "1"), c("b", "2"), c("c", "3")], None));
        assert_eq!(merge_comments(&[c("a", "1")], &loaded["t"].0).len(), 3, "the first page's comments show once");
        edit_loaded(&mut loaded, "b", "two");
        assert_eq!(loaded["t"].0[1]["body"], "two");

        let mut s = App::new();
        s.ui.pages.detail.detail = Some(json!({"viewer": "sparsh", "capabilities": {"edit": {"comment": true}}}));
        assert!(can_edit(&s, &json!({"author": {"login": "Sparsh"}})), "the viewer's own comment");
        assert!(!can_edit(&s, &json!({"author": {"login": "theo"}})));
        assert!(can_edit(&s, &json!({"author": {"login": "theo"}, "canEdit": true})), "the host's word wins");
        s.ui.pages.code.thread_pending = Some((1, EDIT_FAILED));
        start_edit(&mut s, "a", "1");
        assert!(s.ui.pages.code.editing.is_none(), "no other comment opens while one saves");
    }

    fn file<'a>(path: &'a str, lines: &[&'a str]) -> (&'a str, Vec<&'a str>) {
        (path, lines.to_vec())
    }

    fn order<'a>(files: &[(&'a str, Vec<&'a str>)]) -> Vec<&'a str> {
        order_diff_files(files).into_iter().map(|n| files[n].0).collect()
    }

    #[test]
    fn files_go_sources_by_import_then_tests_by_their_source_then_generated() {
        let source = ["src/app.ts", "src/dist.ts", "src/testing.ts"];
        let tests = ["src/__tests__/app.ts", "src/app.spec.tsx", "src/app.test.ts", "test/app.ts", "tests/helpers/app.ts"];
        let generated = ["apps/web/package-lock.json", "dist/app.js", "packages/core/vendor/lib.js", "pnpm-lock.yaml", "public/app.min.js", "src/__snapshots__/app.ts", "src/api.generated.ts", "src/app.test.ts.snap"];
        let all: Vec<&str> = generated.iter().chain(&tests).chain(&source).rev().copied().collect();
        let expected: Vec<&str> = source.iter().chain(&tests).chain(&generated).copied().collect();
        assert_eq!(order(&all.iter().map(|p| file(p, &[])).collect::<Vec<_>>()), expected);

        assert_eq!(order(&[file("src/ui/a.ts", &["import { b } from \"../lib/b\";"]), file("src/lib/b.ts", &[])]), ["src/lib/b.ts", "src/ui/a.ts"], "climbs out of the importer's folder");
        assert_eq!(order(&[file("src/a.ts", &["import { b } from \"./lib\";"]), file("src/lib/index.ts", &[])]), ["src/lib/index.ts", "src/a.ts"], "a folder names its index");
        assert_eq!(order(&[file("src/a.ts", &["import { b } from \"~/lib/b\";"]), file("lib/b.ts", &[])]), ["lib/b.ts", "src/a.ts"], "the last segment when the path does not line up");
        assert_eq!(order(&[file("src/a.ts", &["import { b } from \"~/somewhere/b\";"]), file("one/b.ts", &[]), file("two/b.ts", &[])]), ["one/b.ts", "src/a.ts", "two/b.ts"], "an ambiguous last segment is not guessed");
        assert_eq!(order(&[file("src/a.ts", &["import { b } from \"./b.js\";"]), file("src/b.ts", &[])]), ["src/b.ts", "src/a.ts"], "the .js beside a .ts");
        assert_eq!(order(&[file("src/a.ts", &["const b = require(\"./b\");", "import \"./c\";"]), file("src/b.ts", &[]), file("src/c.ts", &[])]), ["src/b.ts", "src/c.ts", "src/a.ts"], "require and bare imports");
        assert_eq!(order(&[file("src/b.ts", &["import { a } from \"./a\";"]), file("src/a.ts", &["import { b } from \"./b\";"])]), ["src/a.ts", "src/b.ts"], "path order inside a cycle");
        assert_eq!(
            order(&[file("pnpm-lock.yaml", &[]), file("src/a.test.ts", &[]), file("src/__tests__/b.ts", &[]), file("src/a.ts", &["import { b } from \"./b\";"]), file("src/b.ts", &[]), file("dist/a.js", &[])]),
            ["src/b.ts", "src/a.ts", "src/__tests__/b.ts", "src/a.test.ts", "dist/a.js", "pnpm-lock.yaml"],
            "tests follow the order of the sources they cover"
        );
        assert_eq!(order(&[file("src/orphan.spec.ts", &[]), file("src/a.test.ts", &[]), file("src/a.ts", &[])]), ["src/a.ts", "src/a.test.ts", "src/orphan.spec.ts"], "a test without its source goes last");
        assert_eq!(
            order(&[file("src/c.browser.test.tsx", &[]), file("src/b.test.ts", &[]), file("src/a.logic.test.ts", &[]), file("src/c.tsx", &[]), file("src/b.ts", &[]), file("src/a.logic.ts", &[])]),
            ["src/a.logic.ts", "src/b.ts", "src/c.tsx", "src/a.logic.test.ts", "src/b.test.ts", "src/c.browser.test.tsx"],
            "the longest dotted prefix naming a source wins"
        );
    }

    #[test]
    fn each_page_is_ordered_apart_and_the_list_follows_the_tree() {
        assert_eq!(tree_order(&["src/b.ts", "lib/x.ts", "src/a.ts", "README.md"]), [0, 2, 1, 3], "a folder stays together where its first file is");

        let files = rune_kit::diff::parse(&["src/a.test.ts", "src/a.ts", "lib/x.ts"].iter().map(|p| format!("diff --git a/{p} b/{p}\n--- a/{p}\n+++ b/{p}\n@@ -1 +1 @@\n-x\n+y\n")).collect::<String>());
        let paths = |pages: &[usize]| ordered_files(&files, pages).iter().map(|f| f.path()).collect::<Vec<_>>();
        assert_eq!(paths(&[1]), ["src/a.ts", "src/a.test.ts", "lib/x.ts"], "a later page never moves ahead of the first");
        assert_eq!(paths(&[]), ["lib/x.ts", "src/a.ts", "src/a.test.ts"]);

        let entry = json!({"projectId": "p", "host": "github.com", "repository": "o/r", "number": 7});
        let s = showing(&entry);
        let shown: Vec<&str> = s.ui.pages.code.diff.files().iter().map(FileDiff::path).collect();
        assert_eq!(shown, ["apps/desktop-rune/src/main.rs", "apps/desktop-rune/src/pages/mod.rs", "apps/desktop-rune/src/pages/usage.rs"], "the tab shows that order");
    }

    #[test]
    fn hidden_tests_leave_no_changes_only_once_no_page_is_left() {
        let files = rune_kit::diff::parse(&["src/a.test.ts", "src/b.spec.ts"].iter().map(|p| format!("diff --git a/{p} b/{p}\n--- a/{p}\n+++ b/{p}\n@@ -1 +1 @@\n-x\n+y\n")).collect::<String>());
        let mut s = App::new();
        s.client_settings.values.insert(HIDE_TESTS_KEY.into(), json!(false));
        assert!(!no_changes(&s, &files));
        assert!(no_changes(&s, &[]));
        s.client_settings.values.insert(HIDE_TESTS_KEY.into(), json!(true));
        assert!(no_changes(&s, &files), "every file is a hidden test");
        s.ui.pages.code.revealed = Some("src/b.spec.ts".into());
        assert!(!no_changes(&s, &files), "a revealed test stays shown");
        s.ui.pages.code.revealed = None;
        s.ui.pages.code.next = Some("c2".into());
        assert!(!no_changes(&s, &files), "a later page may hold changes");
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

    fn key(key: Key) -> KeyEvent {
        KeyEvent { key, mods: rune::Mods::default() }
    }

    #[test]
    fn find_opens_on_the_chord_and_going_to_a_match_unfolds_its_file() {
        let entry = json!({"projectId": "p", "host": "github.com", "repository": "o/r", "number": 7});
        let mut s = showing(&entry);
        s.client_settings.values.insert("diffFilesCollapsed".into(), json!(true));
        show(&mut s);
        let usage = "apps/desktop-rune/src/pages/usage.rs";
        let f = s.ui.pages.code.diff.file_index(usage).unwrap();
        assert!(s.ui.pages.code.diff.collapsed(f));
        let mut tx = Tx::default();
        assert!(!diff_find::key(&mut s, Host::PullRequest, &key(Key::Escape), &mut tx), "nothing to close yet");
        assert!(diff_find::key(&mut s, Host::PullRequest, &key(Key::Command('f')), &mut tx));
        assert!(s.ui.pages.code.find.is_some() && s.ui.pages.code.find_edit.focused);
        s.ui.pages.code.find_edit = TextEdit::new("hour_label");
        assert!(diff_find::refresh(&mut s, Host::PullRequest));
        assert!(diff_find::key(&mut s, Host::PullRequest, &key(Key::Command('g')), &mut tx));
        assert!(!s.ui.pages.code.diff.collapsed(f), "the match's file opens");
        assert!(s.ui.pages.code.toggled.contains(usage), "through the tab's own folds, so a redraw keeps it open");
        assert!(diff_find::key(&mut s, Host::PullRequest, &key(Key::Escape), &mut tx));
        assert!(s.ui.pages.code.find.is_none());
    }

    #[test]
    fn a_file_asked_for_from_outside_is_scrolled_to_once_it_is_loaded() {
        let entry = json!({"projectId": "p", "host": "github.com", "repository": "o/r", "number": 7});
        let mut s = showing(&entry);
        let usage = "apps/desktop-rune/src/pages/usage.rs";
        s.ui.pages.detail.expanded.insert(usage.into());
        poll(&mut s);
        assert_eq!(s.ui.pages.code.reveal.as_deref(), Some(usage));
        let f = s.ui.pages.code.diff.file_index(usage).unwrap();
        assert!(!s.ui.pages.code.diff.collapsed(f));
        let mut tx = Tx::default();
        assert!(answer_reveal(&mut s, &mut tx));
        assert!(s.ui.pages.code.reveal.is_none(), "answered once");
        s.ui.pages.code.reveal = Some("later.rs".into());
        assert!(!answer_reveal(&mut s, &mut tx), "no page is left to read it from");
        assert_eq!(s.ui.pages.code.reveal.as_deref(), Some("later.rs"), "kept until a page brings it");
    }

    #[test]
    fn a_withheld_file_counts_what_the_host_reported() {
        let files = rune_kit::diff::parse(WITHHELD_PATCH);
        let paths: Vec<&str> = files.iter().map(FileDiff::path).collect();
        assert_eq!(paths, ["assets/icon.png", "data/fixtures/large.json", "src/new_name.rs", "src/copy.rs"], "binary, withheld, renamed and copied files each get a row");
        assert!(matches!(files[2].status, FileStatus::Renamed) && files[2].old_path.as_deref() == Some("src/old_name.rs"));
        assert!(matches!(files[3].status, FileStatus::Copied) && files[3].old_path.as_deref() == Some("src/base.rs"));
        let mut c = Ui::default();
        absorb_omitted(&mut c, &json!({"omittedFileStats": [{"path": "data/fixtures/large.json", "additions": 4210, "deletions": 96}]}));
        assert_eq!(header_counts(&c, &files[1]), (4210, 96), "omittedFileStats stand in for an empty patch");
        assert_eq!(header_counts(&c, &files[0]), (0, 0));
        assert_eq!(header_counts(&c, &files[3]), (1, 1), "a file with a patch keeps its own counts");
    }

    #[test]
    fn the_scroller_path_names_the_view_and_its_scroll_child() {
        assert_eq!(SCROLLER, format!("{VIEW_KEY}/{SCROLL_KEY}"));
    }

    #[test]
    fn a_finished_turn_rereads_the_first_page_and_the_ticks() {
        let entry = json!({"projectId": "p", "host": "github.com", "repository": "o/r", "number": 7});
        let mut s = showing(&entry);
        poll(&mut s);
        s.ui.pages.code.pages = vec![1];
        turn_finished(&mut s);
        assert!(s.ui.pages.code.viewed.refresh, "the viewed ticks read again");
        let p = "src/a.rs";
        let page = json!({"patch": format!("diff --git a/{p} b/{p}\n--- a/{p}\n+++ b/{p}\n@@ -1 +1 @@\n-x\n+y\n"), "truncated": false, "nextCursor": "c2"});
        assert!(merge_first_page(&mut s, &page));
        poll(&mut s);
        let shown: Vec<&str> = s.ui.pages.code.diff.files().iter().map(FileDiff::path).collect();
        assert_eq!(shown, [p]);
        assert!(s.ui.pages.code.pages.is_empty(), "pages read for the old diff go");
        assert_eq!(s.ui.pages.code.next.as_deref(), Some("c2"));

        // The page merge: the same first page again keeps the pages read after it.
        s.ui.pages.code.pages = vec![1];
        s.ui.pages.code.next = None;
        assert!(!merge_first_page(&mut s, &page.clone()));
        poll(&mut s);
        assert_eq!((s.ui.pages.code.pages.as_slice(), s.ui.pages.code.next.as_deref()), (&[1][..], None), "an unchanged first page keeps the later pages");
        let mut moved = page.clone();
        moved["nextCursor"] = json!("c3");
        assert!(merge_first_page(&mut s, &moved), "a changed page replaces them");
        poll(&mut s);
        assert!(s.ui.pages.code.pages.is_empty());
    }

    #[test]
    fn a_page_the_parser_cannot_read_shows_as_text_instead_of_no_changes() {
        assert_eq!(unparsed("Binary files differ\n", &[]), ["Binary files differ\n"]);
        assert!(unparsed("  \n", &[]).is_empty());
        assert!(unparsed("x", &patch()).is_empty());
        let mut s = App::new();
        s.client_settings.values.insert(HIDE_TESTS_KEY.into(), json!(false));
        assert!(no_changes(&s, &[]));
        s.ui.pages.code.raw = vec!["Binary files differ\n".into()];
        assert!(!no_changes(&s, &[]), "the page shows as text instead");
    }

    #[test]
    fn the_file_tree_takes_arrow_keys_for_folders_and_its_width() {
        assert_eq!(resize_step(&Key::Left, 320.0, 1000.0), Some(336.0));
        assert_eq!(resize_step(&Key::Right, 200.0, 1000.0), Some(192.0), "never under the minimum");
        assert_eq!(resize_step(&Key::Home, 400.0, 1000.0), Some(192.0));
        assert_eq!(resize_step(&Key::End, 300.0, 1000.0), Some(560.0));
        assert_eq!(resize_step(&Key::Up, 300.0, 1000.0), None);
        let mut folded = HashSet::new();
        assert!(fold_key(&mut folded, "src", &Key::Left) && folded.contains("src"));
        assert!(!fold_key(&mut folded, "src", &Key::Left), "already folded");
        assert!(fold_key(&mut folded, "src", &Key::Right) && folded.is_empty());
        assert!(!fold_key(&mut folded, "src", &Key::Right), "already open, so the key goes on");
    }
}
