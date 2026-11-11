//! The prompt stash (useComposerStash, ComposerStashBadge, ComposerStashMenu): drafts set aside,
//! newest first, then restored into whichever composer is open or deleted. One stash for all
//! threads, kept on disk beside the drafts.

use rune::{Cursor, El, Key, KeyEvent, col, el, icon, row, spacer, text};
use serde_json::{Value, json};

use super::{Draft, attachments, draft, draft_mut, restored, stored_draft};
use crate::App;
use crate::data;
use crate::icons;
use crate::theme::{self, AlphaMul, T, radius};

const MAX_ENTRIES: usize = 20;
const SNIPPET_CHARS: usize = 90;

#[derive(Default)]
pub struct Stash {
    /// `{id, createdAt, draft}`, newest first, `draft` as `stored_draft` writes it.
    pub entries: Vec<Value>,
    pub open: bool,
    /// The highlighted row while the menu is open.
    pub active: usize,
    loaded: bool,
    dirty: bool,
}

fn has_content(d: &Draft) -> bool {
    !d.edit.text.trim().is_empty() || !d.attachments.is_empty()
}

/// `composer.stash`: sets the draft's prompt, files and chips aside (its model and modes stay).
/// With nothing typed it restores the only entry, or opens the menu to pick one.
pub fn stash(s: &mut App) {
    let Some(d) = draft(s).filter(|d| has_content(d)) else {
        if s.ui.chat.composer.stash.entries.len() == 1 {
            restore(s, 0);
        } else {
            toggle(s);
        }
        return;
    };
    let entry = json!({"id": s5_net::uuid(), "createdAt": s.now_ms, "draft": stored_draft(d)});
    let d = draft_mut(s);
    d.edit.clear();
    d.attachments.clear();
    d.contexts.clear();
    let stash = &mut s.ui.chat.composer.stash;
    stash.entries.insert(0, entry);
    stash.open = false;
    stash.dirty = true;
    if stash.entries.len() > MAX_ENTRIES {
        stash.entries.truncate(MAX_ENTRIES);
        let description = format!("The stash holds {MAX_ENTRIES} prompts; the oldest was removed to make room.");
        crate::shell::palette::toast(s, "Oldest stashed prompt discarded", Some(&description));
    }
}

pub fn toggle(s: &mut App) {
    let stash = &mut s.ui.chat.composer.stash;
    stash.open = !stash.open;
    stash.active = 0;
}

/// Takes entry `index` out of the stash and into the open draft.
pub fn restore(s: &mut App, index: usize) {
    let stash = &mut s.ui.chat.composer.stash;
    if index >= stash.entries.len() {
        return;
    }
    let entry = stash.entries.remove(index);
    stash.open = false;
    stash.active = 0;
    stash.dirty = true;
    if let Some(back) = restored(&json!({"entry": entry["draft"]})).remove("entry") {
        merge(draft_mut(s), back);
    }
}

pub fn delete(s: &mut App, index: usize) {
    let stash = &mut s.ui.chat.composer.stash;
    if index < stash.entries.len() {
        stash.entries.remove(index);
        stash.dirty = true;
    }
    if stash.entries.is_empty() {
        stash.open = false;
    }
    stash.active = stash.active.min(stash.entries.len().saturating_sub(1));
}

/// A restored prompt goes after what is typed, a blank line between (restoreStashEntry); its
/// files and chips join the draft's.
fn merge(d: &mut Draft, back: Draft) {
    if d.edit.text.trim().is_empty() {
        d.edit = back.edit;
    } else if !back.edit.text.is_empty() {
        let end = d.edit.text.trim_end().len();
        d.edit.set_selection(end, d.edit.text.len());
        d.edit.insert("\n\n");
        let mut at = 0;
        for (range, id) in back.edit.pills() {
            d.edit.insert(&back.edit.text[at..range.start]);
            d.edit.insert_pill(id, &back.edit.text[range.clone()]);
            at = range.end;
        }
        d.edit.insert(&back.edit.text[at..]);
    }
    d.edit.set_caret(d.edit.text.len());
    d.edit.focused = true;
    for path in back.attachments {
        if !d.attachments.contains(&path) {
            d.attachments.push(path);
        }
    }
    d.contexts.extend(back.contexts);
}

/// The open menu's keys (ComposerStashMenu): arrows move, Enter restores, Mod+Backspace deletes.
pub fn on_key(s: &mut App, e: &KeyEvent) -> bool {
    let stash = &mut s.ui.chat.composer.stash;
    if !stash.open {
        return false;
    }
    let count = stash.entries.len();
    let at = stash.active.min(count.saturating_sub(1));
    match e.key {
        Key::Escape => stash.open = false,
        Key::Down if count > 0 => stash.active = (at + 1) % count,
        Key::Up if count > 0 => stash.active = (at + count - 1) % count,
        Key::Enter if count > 0 && !e.mods.shift => restore(s, at),
        Key::Backspace if count > 0 && e.mods.cmd => delete(s, at),
        _ => return false,
    }
    true
}

/// Reads the stash once the shell has loaded, then writes it after each change.
pub(super) fn persist(s: &mut App) -> bool {
    let path = crate::settings::client::sibling_path("composer-stash.json");
    if !s.ui.chat.composer.stash.loaded {
        if !s.state().shell_ready {
            return false;
        }
        let disk = std::fs::read_to_string(&path).ok().and_then(|t| serde_json::from_str::<Value>(&t).ok());
        let stash = &mut s.ui.chat.composer.stash;
        stash.loaded = true;
        let Some(Value::Array(disk)) = disk else { return false };
        // Anything stashed before the read is newer.
        stash.entries.extend(disk);
        stash.entries.truncate(MAX_ENTRIES);
        return true;
    }
    let stash = &mut s.ui.chat.composer.stash;
    if stash.dirty {
        stash.dirty = false;
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Err(e) = std::fs::write(&path, Value::Array(stash.entries.clone()).to_string()) {
            eprintln!("composer: saving the stash failed: {e}");
        }
    }
    false
}

/// stashEntrySnippet: the prompt on one line, else what it carries.
fn snippet(draft: &Value) -> String {
    let text = data::s(draft, "prompt").split_whitespace().collect::<Vec<_>>().join(" ");
    if !text.is_empty() {
        return if text.chars().count() > SNIPPET_CHARS { format!("{}…", text.chars().take(SNIPPET_CHARS).collect::<String>()) } else { text };
    }
    let paths: Vec<&str> = draft["attachments"].as_array().into_iter().flatten().filter_map(Value::as_str).collect();
    let images = paths.iter().filter(|p| attachments::is_image(p)).count();
    let label = match (images, paths.len() - images) {
        (0, 0) => return "(empty)".to_owned(),
        (_, 0) => "image",
        (0, _) => "file",
        _ => "attachment",
    };
    format!("({} {label}{})", paths.len(), if paths.len() == 1 { "" } else { "s" })
}

fn shoulder(t: &T) -> El {
    row()
        .corners([radius::XL2, radius::XL2, 0.0, 0.0])
        .fill(if t.dark { t.c.surface_raised } else { t.c.card })
        .border(1.0, if t.dark { rune::Color::WHITE.alpha(0.05) } else { rune::Color::BLACK.alpha(0.08) })
}

/// The Stash tab on the composer's shoulder (ComposerStashBadge), while anything is stashed.
pub fn badge(s: &App, t: &T) -> Option<El> {
    let stash = &s.ui.chat.composer.stash;
    if stash.entries.is_empty() {
        return None;
    }
    let ink = if stash.open { t.c.foreground } else { t.c.muted_foreground };
    let count = stash.entries.len();
    Some(
        shoulder(t)
            .key("composer-stash-badge")
            .px(12.0)
            .pt(6.0)
            .pb(17.0 + 6.0)
            .gap(6.0)
            .center_y()
            .cursor(Cursor::Pointer)
            .role("button")
            .label(format!("Stashed prompts: {count}. Open stash."))
            .child(icon(&icons::BOOKMARK, 14.0).color(ink))
            .child(text("Stash").font(theme::weight(theme::TEXT_XS, 500.0)).color(ink))
            .child(text(count.to_string()).font(theme::TEXT_XS.features("'tnum' 1")).color(t.c.muted_foreground))
            .on_press(toggle),
    )
}

/// The open stash drawer above the composer (ComposerStashMenu).
pub fn menu(s: &App, t: &T) -> Option<El> {
    let stash = &s.ui.chat.composer.stash;
    if !stash.open {
        return None;
    }
    let muted = t.c.muted_foreground;
    let header = row()
        .key("composer-stash-close")
        .w_full()
        .h(28.0)
        .px(8.0)
        .gap(8.0)
        .center_y()
        .cursor(Cursor::Pointer)
        .role("button")
        .label("Close stash")
        .child(icon(&icons::BOOKMARK, 14.0).color(muted))
        .child(text("Stash").font(theme::TEXT_XS).color(muted))
        .child(spacer())
        .child(text(stash.entries.len().to_string()).font(theme::TEXT_XS.features("'tnum' 1")).color(muted))
        .child(icon(&icons::CHEVRON_DOWN, 14.0).color(muted))
        .on_press(toggle);
    let mut rows = col().w_full();
    if stash.entries.is_empty() {
        let shortcut = crate::shell::palette::shortcut_label(s, "composer.stash");
        let hint = if shortcut.is_empty() { String::new() } else { format!(" Press {shortcut} with a prompt in the composer to stash it.") };
        rows = rows.child(row().w_full().px(8.0).py(6.0).child(text(format!("Nothing stashed yet.{hint}")).font(theme::TEXT_XS).color(muted).wrap()));
    }
    let active = stash.active.min(stash.entries.len().saturating_sub(1));
    for (i, entry) in stash.entries.iter().enumerate() {
        let draft = &entry["draft"];
        let label = snippet(draft);
        let files = draft["attachments"].as_array().map_or(0, Vec::len);
        let mut restore_target = row()
            .key(format!("composer-stash-{i}"))
            .h(28.0)
            .gap(8.0)
            .grow(1.0)
            .shrink()
            .min_w(0.0)
            .center_y()
            .cursor(Cursor::Pointer)
            .label(format!("Restore stashed prompt: {label}"))
            .child(icon(&icons::FILE_TEXT, 14.0).color(muted))
            .child(text(label).font(theme::TEXT_XS).color(t.c.foreground.alpha_mul(0.8)).truncate().grow(1.0).shrink().min_w(0.0));
        if files > 0 {
            restore_target = restore_target.child(row().gap(4.0).center_y().child(icon(&icons::FILE, 12.0).color(muted)).child(text(files.to_string()).font(theme::TEXT_XS).color(muted)));
        }
        if let Some(at) = entry["createdAt"].as_i64() {
            let age = crate::shell::sidebar::logic::relative_label(at, s.now_ms);
            restore_target = restore_target.child(text(age).font(theme::TEXT_XS.features("'tnum' 1")).color(muted));
        }
        let delete_button = el()
            .key(format!("composer-stash-delete-{i}"))
            .size(20.0, 20.0)
            .center()
            .radius(radius::SM)
            .hover(t.c.accent)
            .cursor(Cursor::Pointer)
            .label("Delete stashed prompt")
            .child(icon(&icons::X, 12.0).color(muted))
            .on_press(move |s: &mut App| delete(s, i));
        let mut r = row()
            .w_full()
            .pl(8.0)
            .pr(4.0)
            .gap(8.0)
            .center_y()
            .radius(theme::layout::CONTROL_RADIUS)
            .on_hover(move |s: &mut App| s.ui.chat.composer.stash.active = i)
            .child(restore_target.on_press(move |s: &mut App| restore(s, i)))
            .child(delete_button);
        if i == active {
            r = r.fill(t.c.accent);
        }
        rows = rows.child(r);
    }
    let rows = col().w_full().max_h(280.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(rows);
    Some(col().key("composer-stash-menu").w_full().px(8.0).pt(4.0).pb(8.0 + 17.0).child(header).child(rows).corners([radius::XL2, radius::XL2, 0.0, 0.0]).fill(if t.dark { t.c.surface_raised } else { t.c.card }).border(1.0, if t.dark { rune::Color::WHITE.alpha(0.05) } else { rune::Color::BLACK.alpha(0.08) }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Route;

    fn on_thread(s: &mut App) {
        let id = data::s(&s.state().threads[0], "id").to_string();
        s.navigate(Route::Thread(id));
    }

    #[test]
    fn stashing_clears_the_prompt_and_restoring_appends_it_after_a_blank_line() {
        let mut s = App::new();
        on_thread(&mut s);
        let record = super::super::chips::terminal("t", "Terminal 1", 2, 2, "ls");
        let d = draft_mut(&mut s);
        d.edit.insert("see ");
        d.edit.insert_pill(super::super::chips::pill_id(&record), "Terminal 1 line 2");
        d.contexts.push(record.clone());
        d.attachments.push("/tmp/a.txt".into());
        d.runtime_mode = Some("approval-required".into());
        stash(&mut s);
        let d = draft(&s).unwrap();
        assert!(d.edit.text.is_empty() && d.attachments.is_empty() && d.contexts.is_empty());
        assert_eq!(d.runtime_mode.as_deref(), Some("approval-required"), "the model and modes stay");
        assert_eq!(s.ui.chat.composer.stash.entries.len(), 1);

        draft_mut(&mut s).edit.insert("now  \n");
        restore(&mut s, 0);
        let d = draft(&s).unwrap();
        assert_eq!(d.edit.text, "now\n\nsee Terminal 1 line 2");
        assert_eq!(d.edit.pills().map(|(r, _)| r).collect::<Vec<_>>(), [9..26]);
        assert_eq!(d.attachments, ["/tmp/a.txt"]);
        assert_eq!(d.contexts, [record]);
        assert!(s.ui.chat.composer.stash.entries.is_empty());
    }

    #[test]
    fn an_empty_composer_restores_a_lone_entry_or_opens_the_menu() {
        let mut s = App::new();
        on_thread(&mut s);
        stash(&mut s);
        assert!(s.ui.chat.composer.stash.open, "nothing to stash opens the (empty) menu");
        toggle(&mut s);
        draft_mut(&mut s).edit.insert("one");
        stash(&mut s);
        stash(&mut s);
        assert_eq!(draft(&s).unwrap().edit.text, "one");
        stash(&mut s);
        draft_mut(&mut s).edit.insert("two");
        stash(&mut s);
        stash(&mut s);
        assert!(s.ui.chat.composer.stash.open && draft(&s).unwrap().edit.text.is_empty());
        let down = KeyEvent { key: Key::Down, mods: rune::Mods::default() };
        assert!(on_key(&mut s, &down));
        let delete_key = KeyEvent { key: Key::Backspace, mods: rune::Mods { cmd: true, ..Default::default() } };
        assert!(on_key(&mut s, &delete_key));
        assert_eq!(s.ui.chat.composer.stash.entries.iter().map(|e| data::s(&e["draft"], "prompt")).collect::<Vec<_>>(), ["two"]);
    }

    #[test]
    fn the_stash_keeps_the_newest_twenty() {
        let mut s = App::new();
        on_thread(&mut s);
        for i in 0..=MAX_ENTRIES {
            draft_mut(&mut s).edit.insert(&format!("prompt {i}"));
            stash(&mut s);
        }
        let entries = &s.ui.chat.composer.stash.entries;
        assert_eq!(entries.len(), MAX_ENTRIES);
        assert_eq!(data::s(&entries[0]["draft"], "prompt"), "prompt 20");
        assert_eq!(data::s(&entries[MAX_ENTRIES - 1]["draft"], "prompt"), "prompt 1");
    }

    #[test]
    fn snippets_fold_whitespace_and_name_what_an_empty_prompt_carries() {
        assert_eq!(snippet(&json!({"prompt": "  fix\n\nthe   bug "})), "fix the bug");
        assert_eq!(snippet(&json!({"prompt": "x".repeat(95)})), format!("{}…", "x".repeat(90)));
        assert_eq!(snippet(&json!({"prompt": "", "attachments": []})), "(empty)");
        assert_eq!(snippet(&json!({"prompt": "", "attachments": ["/a/shot.png"]})), "(1 image)");
        assert_eq!(snippet(&json!({"prompt": "", "attachments": ["/a/b.pdf", "/a/c.txt"]})), "(2 files)");
        assert_eq!(snippet(&json!({"prompt": "", "attachments": ["/a/shot.png", "/a/c.txt"]})), "(2 attachments)");
    }
}
