//! The main column: topbar, then either the home/draft screen or a thread's timeline, with the
//! composer docked at the bottom. `timeline` is one worker's; `topbar`, `composer`, `home` and
//! `details` (the floating thread details card) are another's.

pub mod composer;
pub mod details;
pub mod guest;
pub mod home;
pub mod server_update;
pub mod side;
pub mod snapshot;
pub mod subagent_bar;
pub mod thread_title;
pub mod timeline;
pub mod topbar;
pub mod triage;

use rune::{Cx, El, Key, KeyEvent, Mods, Tx, col, icon, row, text};

use crate::theme::{self, AlphaMul, T};
use crate::{App, Route, icons};

#[derive(Default)]
pub struct Ui {
    pub timeline: timeline::Ui,
    pub composer: composer::Ui,
    pub home: home::Ui,
    pub details: details::Ui,
    pub topbar: topbar::Ui,
    pub server_update: server_update::Updates,
    pub side: side::State,
    /// The composer's notice stack (ComposerBannerStack).
    pub banners: composer::banners::State,
    /// The remembered route was restored (or found stale) once the shell loaded.
    route_restored: bool,
    /// A file drag is over the chat (WorkspaceFileDropOverlay shows).
    drop_over: bool,
    pub guest: guest::State,
    pub snapshot: snapshot::State,
}

const LAST_ROUTE: &str = "lastRoute";

/// Web reopens the last thread on restart. The main route is kept in client settings; once the
/// shell has loaded, a launch still on the bare home route returns to it if it still exists.
pub fn remember_route(s: &mut App) -> bool {
    if !matches!(s.src, crate::data::Source::Live(_)) || !s.state().shell_ready {
        return false;
    }
    if !s.ui.chat.route_restored {
        s.ui.chat.route_restored = true;
        let stored = s.client_settings.values.get(LAST_ROUTE).and_then(decode_route);
        if s.route == (Route::Home { project_id: None })
            && let Some(route) = stored.filter(|r| route_exists(s, r))
        {
            s.navigate(route);
            return true;
        }
    }
    let current = encode_route(s.frame_route());
    if current.is_some() && s.client_settings.values.get(LAST_ROUTE) != current.as_ref() {
        s.client_settings.values.insert(LAST_ROUTE.into(), current.unwrap_or_default());
        s.client_settings.save();
    }
    false
}

fn encode_route(route: &Route) -> Option<serde_json::Value> {
    match route {
        Route::Thread(id) => Some(serde_json::json!({"thread": id})),
        Route::Home { project_id } => Some(serde_json::json!({"home": project_id})),
        Route::PullRequests => Some(serde_json::json!({"pullRequests": true})),
        Route::Settings(_) | Route::Usage => None,
    }
}

fn decode_route(v: &serde_json::Value) -> Option<Route> {
    if let Some(id) = v["thread"].as_str() {
        return Some(Route::Thread(id.to_owned()));
    }
    if v["pullRequests"] == true {
        return Some(Route::PullRequests);
    }
    v.get("home").map(|p| Route::Home { project_id: p.as_str().map(str::to_owned) })
}

fn route_exists(s: &App, route: &Route) -> bool {
    let state = s.state();
    let has = |list: &[serde_json::Value], id: &str| list.iter().any(|v| crate::data::s(v, "id") == id);
    match route {
        Route::Thread(id) => has(&state.threads, id) || has(&state.archived, id),
        Route::Home { project_id: Some(id) } => has(&state.projects, id),
        _ => true,
    }
}

pub fn view(s: &App, cx: &mut Cx<App>, t: &T) -> El {
    let main = col().key("main").grow(1.0).shrink().min_w(0.0).h_full().fill(t.c.background);
    // Settings opened directly (no route to return to) sits over an empty main column.
    if matches!(s.frame_route(), Route::Settings(_)) {
        return main;
    }
    if let Route::Thread(id) = s.frame_route()
        && crate::not_found::missing_thread(s, id)
    {
        return main.child(crate::not_found::no_active_thread(t));
    }
    let (w, h) = cx.layout_with("chat-body", |r| (r[2].round(), r[3].round())).unwrap_or((1182.0, 848.0));
    let shown = details::shown(s, w, h);
    let lane = details::lane_max(w, h, shown != details::Shown::Inline, t.chat_w);
    // Under the settings modal the frame keeps showing the route settings was opened from.
    let body = match s.frame_route() {
        Route::Thread(id) if !timeline::is_empty(s, id) => timeline::view(s, cx, t, id, &lane),
        _ => home::view(s, cx, t, &lane),
    };
    // A provider-native subagent takes no messages; its bar stands in for the composer.
    let bar = subagent_bar::view(s, cx, t, &lane);
    let takes_files = bar.is_none();
    // The composer, details card and their menus float over the body.
    let mut stack = col()
        .key("chat-body")
        .grow(1.0)
        .min_h(0.0)
        .w_full()
        .child(body)
        .children(timeline::scroll_to_end(s, cx, t, &lane))
        .children(timeline::minimap(s, cx, t, &lane))
        .child(timeline::announcer(s))
        .children(timeline::banners::lane(s, t, &lane))
        .children(timeline::banners::stack(s, cx, t, &lane))
        .child(bar.unwrap_or_else(|| composer::view(s, cx, t, &lane)))
        .children(details::view(s, cx, t, shown, w))
        .children({
            let bw = details::WIDTH.min(w - 2.0 * details::GAP);
            timeline::find::bar(s, t, w - bw - details::GAP, details::GAP, bw)
        })
        .children(details::menu(s, t))
        .children(composer::overlays(s, cx, t))
        .children(timeline::actions::overlays(s, t))
        .children(timeline::background_setup::overlays(s, t))
        .children(timeline::plan::overlays(s, t))
        .children(timeline::chip::overlay(s, t))
        .children(timeline::chip::quote_toolbar(s, cx, t))
        .children(timeline::attachments::lightbox(s))
        .children(snapshot::popover(s, t))
        .children(topbar::overlays(s, t));
    if takes_files {
        stack = stack
            .on_drop_files(|s: &mut App, files: &[std::path::PathBuf], _: &mut Tx| {
                s.ui.chat.drop_over = false;
                composer::drop_files(s, files);
            })
            .on_drop_hover(|s: &mut App, over, _: &mut Tx| s.ui.chat.drop_over = over)
            .children(s.ui.chat.drop_over.then(|| drop_overlay(t)));
    }
    main.child(topbar::view(s, cx, t))
        .child(stack)
        .children(crate::panel::drawer::view(s, cx, t))
}

/// `canOperateThread`: the connection may change threads (`orchestration:operate`). Unknown until
/// its session answers, which leaves it to the server; a session Settings read counts too.
pub fn can_operate(s: &App) -> bool {
    const OPERATE: &str = "orchestration:operate";
    details::grants(s, OPERATE) && !matches!(crate::settings::permissions::access_on(s, &s.ui.env.active, OPERATE), Some(Err(None)))
}

/// WorkspaceFileDropOverlay: the whole chat takes a file drag for the composer.
fn drop_overlay(t: &T) -> El {
    col()
        .key("chat-drop-overlay")
        .pin(Some(8.0), Some(8.0), Some(8.0), Some(8.0))
        .radius(16.0)
        .border(2.0, t.c.primary.alpha_mul(0.6))
        .border_dash(6.0, 4.0)
        .fill(t.c.primary.alpha_mul(0.035))
        .center()
        .child(
            row()
                .gap(8.0)
                .center_y()
                .px(16.0)
                .py(10.0)
                .radius(theme::radius::FULL)
                .border(1.0, t.c.primary.alpha_mul(0.25))
                .fill(t.c.background.alpha_mul(0.95))
                .shadow(4.0, 12.0, rune::Color::BLACK.alpha(0.1))
                .role("status")
                .child(icon(&icons::PAPERCLIP, 16.0).color(t.c.primary))
                .child(text("Drop files to attach").font(theme::SM_MEDIUM).color(t.c.foreground)),
        )
}

/// Chat keys no focused control took: find in thread, then a question's 1-9 shortcuts, the
/// composer's chords, and last a stray keystroke or paste for the composer.
pub fn on_key(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    timeline::attachments::on_key(s, e)
        || timeline::find::on_key(s, e, tx)
        || timeline::question::on_key(s, e, tx)
        || composer::shortcut(s, e)
        || details::shortcut(s, e)
        || type_to_composer(s, e, tx)
}

/// shouldTypeToFocusComposer: one printable character with no modifier but Shift.
fn typed(key: &Key, mods: Mods) -> Option<&str> {
    match key {
        Key::Text(c) if !(mods.cmd || mods.ctrl || mods.alt || mods.logo) && c.chars().count() == 1 => Some(c.as_str()),
        _ => None,
    }
}

/// composerFocusRedirect / usePasteToFocusComposer: a key or a paste made while nothing has
/// focus goes to the end of the composer, unless a popup, the terminal or the model picker holds it.
fn type_to_composer(s: &mut App, e: &KeyEvent, tx: &mut Tx) -> bool {
    if tx.focused().is_some()
        || !matches!(s.route, Route::Thread(_) | Route::Home { .. })
        || s.ui.shell.palette.open
        || s.ui.chat.composer.picker.open
        || crate::panel::terminal_active(s)
    {
        return false;
    }
    if let Some(c) = typed(&e.key, e.mods) {
        let c = c.to_owned();
        return composer::insert_at_end(s, &c);
    }
    if e.key == Key::Command('v') && !e.mods.alt {
        // Mod+Shift+V is Paste as Text (isPasteAsTextShortcut): no folding into a file.
        let as_text = e.mods.shift;
        tx.read_clipboard(move |s: &mut App, content: &rune::ClipboardContent, _: &mut Tx| {
            if content.files().is_empty() {
                composer::paste_at_end(s, content.text(), content.html(), as_text);
            }
        });
        return true;
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_remembered_route_round_trips() {
        for route in [Route::Thread("t1".into()), Route::Home { project_id: Some("p".into()) }, Route::Home { project_id: None }, Route::PullRequests] {
            assert_eq!(encode_route(&route).as_ref().and_then(decode_route), Some(route));
        }
        assert_eq!(encode_route(&Route::Usage), None);
        assert_eq!(decode_route(&serde_json::json!("garbage")), None);
    }

    #[test]
    fn only_a_plain_printable_key_types_into_the_composer() {
        let plain = Mods::default();
        let shift = Mods { shift: true, ..Mods::default() };
        assert_eq!(typed(&Key::Text("a".into()), plain), Some("a"));
        assert_eq!(typed(&Key::Text("A".into()), shift), Some("A"));
        assert_eq!(typed(&Key::Text("é".into()), plain), Some("é"));
        assert_eq!(typed(&Key::Text(" ".into()), Mods { cmd: true, ..Mods::default() }), None, "Cmd-Space");
        assert_eq!(typed(&Key::Text("k".into()), Mods { ctrl: true, ..Mods::default() }), None);
        assert_eq!(typed(&Key::Text("ø".into()), Mods { alt: true, ..Mods::default() }), None);
        assert_eq!(typed(&Key::Text("ab".into()), plain), None, "composed text is not one key");
        assert_eq!(typed(&Key::Enter, plain), None);
    }

    #[test]
    fn a_stray_key_lands_at_the_end_of_the_prompt_and_focuses_it() {
        let mut s = App::new();
        s.route = Route::Thread("t1".into());
        s.ui.chat.composer.drafts.entry("t1".into()).or_default().edit = rune::TextEdit::new("hi ");
        let mut tx = Tx::default();
        let e = KeyEvent::from(Key::Text("x".into()));
        assert!(type_to_composer(&mut s, &e, &mut tx));
        let d = &s.ui.chat.composer.drafts["t1"];
        assert_eq!(d.edit.text, "hi x");
        assert!(d.edit.focused);
        // A stray paste: long text folds into a file, unless Mod+Shift+V pasted it as text.
        let long = "y".repeat(composer::attachments::PASTED_TEXT_THRESHOLD);
        assert!(composer::paste_at_end(&mut s, &long, None, false));
        let d = &s.ui.chat.composer.drafts["t1"];
        assert_eq!((d.edit.text.as_str(), d.attachments.len()), ("hi x", 1), "FAILED: a long stray paste folds into pasted-text.txt");
        assert!(composer::paste_at_end(&mut s, &long, None, true));
        let d = &s.ui.chat.composer.drafts["t1"];
        assert_eq!((d.edit.text.len(), d.attachments.len()), (4 + long.len(), 1), "FAILED: Paste as Text lands verbatim at the end");
        s.ui.shell.palette.open = true;
        assert!(!type_to_composer(&mut s, &e, &mut tx), "an open palette keeps its keys");
    }
}
