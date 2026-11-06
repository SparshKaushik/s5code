//! The thread details card (ThreadDetailsCard): a glass card floating at the top right of the
//! chat area with the run context, editor and script actions, and version control. It also owns
//! the chat lane geometry (chatCanvasLayout.ts, threadDetailsCardLayout.ts), since the card is
//! what narrows the lane.

use rune::tok::TextStyle;
use rune::{Align, Color, Cursor, Cx, El, Side, col, el, icon, row, text};
use serde_json::{Value, json};

use crate::chat::composer;
use crate::data;
use crate::icons;
use crate::theme::{self, AlphaMul, T, radius};
use crate::App;

#[derive(Default)]
pub struct Ui {
    /// The user closed the card from the topbar toggle.
    pub hidden: bool,
    pub menu: Option<Menu>,
}

#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Menu {
    Editor,
    Git,
    Branch,
}

pub const GAP: f32 = 12.0;
pub const WIDTH: f32 = theme::layout::THREAD_DETAILS_W;
const CLEARANCE: f32 = 32.0;
const LANE_PADDING: f32 = 48.0;
const MIN_CHAT: f32 = 640.0;

/// Where the chat column sits in a chat area `w` by `h`, and whether the card shows.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Lane {
    pub left: f32,
    pub width: f32,
    pub card: bool,
}

pub fn lane(w: f32, h: f32, hidden: bool) -> Lane {
    let x = w - WIDTH - GAP;
    let card = !hidden && x - CLEARANCE - LANE_PADDING >= MIN_CHAT && h - 2.0 * GAP >= 160.0;
    let centered = (w - LANE_PADDING * 2.0).min(theme::layout::CHAT_MAX_W).max(0.0);
    let right = if card { x - CLEARANCE } else { w - LANE_PADDING };
    let width = centered.min(right - LANE_PADDING).max(0.0);
    let left = ((w - width) / 2.0).min(right - width).max(LANE_PADDING);
    Lane { left, width, card }
}

/// The card's row type: 13px medium.
const ROW: TextStyle = TextStyle::new(13.0, theme::MEDIUM).lh(19.5);

fn cwd(s: &App) -> Option<String> {
    let c = composer::ctx(s);
    let worktree = c.thread.map(|t| data::s(t, "worktreePath")).filter(|p| !p.is_empty());
    worktree.or_else(|| c.project.map(|p| data::s(p, "workspaceRoot"))).filter(|p| !p.is_empty()).map(str::to_string)
}

/// The checkout's `{local, remote}` status (`subscribeVcsStatus`); `local` is null until it loads.
fn vcs<'a>(s: &'a App) -> Option<&'a Value> {
    s.state().vcs.get(&cwd(s)?).filter(|v| v["local"].is_object())
}

/// Subscribes to the framed checkout's status, as the card's `useVcsStatus` does.
pub fn watch(s: &mut App) {
    if let Some(cwd) = cwd(s)
        && let Some(net) = s.src.net()
    {
        net.watch_vcs(&cwd);
    }
}

/// A card row: icon, label, then trailing content. `split` leaves room for a 32-point chevron.
fn action(t: &T, key: &str, glyph: El, label: &str, color: Color) -> El {
    row()
        .key(key.to_string())
        .h(32.0)
        .pl(11.0)
        .pr(10.0)
        .gap(10.0)
        .center_y()
        .radius(theme::layout::CONTROL_RADIUS)
        .hover(t.c.accent)
        .cursor(Cursor::Pointer)
        .label(label.to_string())
        .child(glyph)
        .child(text(label.to_string()).font(ROW).color(color).truncate().grow(1.0).shrink().min_w(0.0))
}

fn split(t: &T, key: &str, main: El, label: &str, menu: Menu) -> El {
    row()
        .w_full()
        .gap(1.0)
        .child(main.w(229.0).corners([8.0, 0.0, 0.0, 8.0]))
        .child(
            el().key(key.to_string())
                .size(32.0, 32.0)
                .center()
                .corners([0.0, 8.0, 8.0, 0.0])
                .hover(t.c.accent)
                .cursor(Cursor::Pointer)
                .label(label.to_string())
                .help(label.to_string())
                .child(icon(&icons::CHEVRON_DOWN, 16.0).color(t.c.muted_foreground))
                .on_press(move |s: &mut App| toggle_menu(s, menu)),
        )
}

fn toggle_menu(s: &mut App, menu: Menu) {
    let ui = &mut s.ui.chat.details;
    ui.menu = if ui.menu == Some(menu) { None } else { Some(menu) };
}

fn open_in_editor(s: &mut App, editor: Option<String>) {
    let (Some(cwd), editor) = (cwd(s), editor.or_else(|| editors(s).first().map(|e| e.0.clone()))) else { return };
    let Some(editor) = editor else { return };
    if let Some(net) = s.src.net() {
        net.request("shell.openInEditor", json!({"cwd": cwd, "editor": editor}));
    }
}

/// `(id, label)` for each editor the server found.
fn editors(s: &App) -> Vec<(String, String)> {
    s.state()
        .config
        .as_ref()
        .and_then(|c| c["availableEditors"].as_array())
        .map(|v| {
            v.iter()
                .map(|e| match e.as_str() {
                    Some(id) => (id.to_string(), id.to_string()),
                    None => (data::s(e, "id").to_string(), data::s(e, "label").to_string()),
                })
                .collect()
        })
        .unwrap_or_default()
}

pub fn view(s: &App, _cx: &mut Cx<App>, t: &T, lane: &Lane, w: f32) -> Option<El> {
    if !lane.card {
        return None;
    }
    let muted = t.c.muted_foreground;
    let fg = t.c.foreground.alpha_mul(0.8);
    let env = s.state().config.as_ref().map(|c| data::s(&c["environment"], "label").to_string()).unwrap_or_default();
    let thread = composer::ctx(s).thread;
    let worktree = thread.is_some_and(|t| !data::s(t, "worktreePath").is_empty());
    let mode = if worktree { "Worktree" } else { "Local checkout" };
    // A draft can still choose where it runs, so its row is the env mode picker.
    let draft = thread.is_none();
    let mut run_context = row()
        .key("details-env")
        .h(32.0)
        .pl(10.0)
        .pr(if draft { 8.0 } else { 11.0 })
        .gap(9.0)
        .center_y()
        .label(format!("Run context: {env}, {mode}"))
        .child(icon(&icons::SERVER, 18.0).color(muted))
        .child(text(env).font(ROW).color(if draft { fg } else { muted }).truncate().grow(1.0).shrink().min_w(0.0))
        .child(icon(if worktree { &icons::GIT_FORK } else { &icons::FOLDER }, 16.0).color(muted).ml(1.0).help(mode));
    if draft {
        run_context = run_context.radius(theme::layout::CONTROL_RADIUS).hover(t.c.accent).cursor(Cursor::Pointer).role("button").child(icon(&icons::CHEVRON_DOWN, 16.0).color(muted).ml(9.0));
    }
    let editor = split(
        t,
        "details-editor-menu",
        {
            let open = action(t, "details-editor", icon(&icons::SQUARE_ARROW_OUT_UP_RIGHT, 16.0).color(muted), "Open in editor", fg);
            // No editor found: the button is disabled (opacity-64), the chooser still opens.
            if editors(s).is_empty() { open.opacity(0.64) } else { open.on_press(|s: &mut App| open_in_editor(s, None)) }
        },
        "Choose editor",
        Menu::Editor,
    );
    let script = action(t, "details-script", icon(&icons::PLUS, 14.0).color(fg), "Add project script", fg).gap(12.0).w_full();
    let general = col().w_full().padding(8.0).pb(10.0).child(run_context).child(editor).child(script);

    let mut card = col()
        .key("thread-details")
        .w(WIDTH)
        .absolute(w - WIDTH - GAP, GAP)
        .radius(radius::XL2)
        .fill(t.c.popover.alpha_mul(0.836))
        .border(1.0, if t.dark { Color::WHITE.alpha(0.1) } else { Color::BLACK.alpha(0.08) })
        .shadow(8.0, 24.0, Color::BLACK.alpha(if t.dark { 0.32 } else { 0.08 }))
        .padding(1.0)
        .child(general);

    if let Some(v) = vcs(s).map(|v| &v["local"]).filter(|v| v["isRepo"] == true) {
        let branch = data::s(v, "refName").to_string();
        let mut section = col()
            .w_full()
            .px(8.0)
            .pt(9.0)
            .pb(10.0)
            .child(el().absolute(0.0, 0.0).w_full().h(1.0).fill(t.c.border.alpha_mul(0.65)))
            .child(
                action(t, "details-branch", icon(&icons::GIT_BRANCH, 16.0).color(muted), &branch, fg)
                    .w_full()
                    .pr(8.0)
                    .role("combobox")
                    .child(icon(&icons::CHEVRON_DOWN, 16.0).color(muted))
                    .on_press(|s: &mut App| toggle_menu(s, Menu::Branch)),
            );
        if draft {
            return Some(card.child(section));
        }
        if v["hasPrimaryRemote"] != true {
            section = section.child(split(
                t,
                "details-git-menu",
                action(t, "details-publish", icon(&icons::CLOUD_UPLOAD, 16.0).color(muted), "Publish repository", fg),
                "Git action options",
                Menu::Git,
            ));
        }
        let (add, del) = (v["workingTree"]["insertions"].as_u64().unwrap_or(0), v["workingTree"]["deletions"].as_u64().unwrap_or(0));
        let stat = theme::mono(theme::XS_MEDIUM);
        section = section.child(
            action(t, "details-changes", icon(&icons::FILE_DIFF, 16.0).color(muted), "Changes", fg)
                .w_full()
                .child(row().gap(6.0).child(text(format!("+{add}")).font(stat).color(muted)).child(text(format!("-{del}")).font(stat).color(muted)))
                .on_press(|s: &mut App| s.ui.shell.panel_open = true),
        );
        card = card.child(section);
    }
    Some(card)
}

/// The open card menu, anchored under its trigger. Attach after the card so it draws above.
pub fn menu(s: &App, t: &T) -> Option<El> {
    let which = s.ui.chat.details.menu?;
    let (anchor, items): (&str, Vec<(String, String)>) = match which {
        Menu::Editor => {
            let list = editors(s);
            ("details-editor-menu", if list.is_empty() { vec![(String::new(), "No editors found".into())] } else { list })
        }
        Menu::Git => ("details-git-menu", vec![("publish".into(), "Publish repository".into()), ("commit".into(), "Commit".into())]),
        Menu::Branch => {
            let branch = vcs(s).map(|v| data::s(&v["local"], "refName").to_string()).unwrap_or_default();
            ("details-branch", vec![(branch.clone(), branch)])
        }
    };
    let mut list = col().key("details-menu").min_w(200.0).padding(4.0).radius(radius::LG).fill(t.c.popover).border(1.0, t.c.border).shadow(8.0, 24.0, Color::BLACK.alpha(0.3));
    for (id, label) in items {
        let enabled = !id.is_empty();
        let mut item = row().h(32.0).px(8.0).center_y().radius(radius::MD).child(text(label).font(theme::TEXT_SM).color(if enabled { t.c.foreground } else { t.c.muted_foreground }));
        if enabled {
            item = item.hover(t.c.accent).cursor(Cursor::Pointer).on_press(move |s: &mut App| {
                s.ui.chat.details.menu = None;
                if which == Menu::Editor {
                    open_in_editor(s, Some(id.clone()));
                }
            });
        }
        list = list.child(item);
    }
    Some(list.anchored(anchor, Side::Bottom, Align::End, 4.0).modal(true, |s: &mut App| s.ui.chat.details.menu = None))
}

#[cfg(test)]
mod tests {
    use super::lane;

    #[test]
    fn lane_matches_web_layout() {
        // 1440 window minus the 256 sidebar and its border: the web lane sits at x=122, 736 wide.
        let l = lane(1182.0, 848.0, false);
        assert!(l.card);
        assert_eq!((l.left, l.width), (122.0, 736.0));
        // Hidden card: centered.
        let l = lane(1182.0, 848.0, true);
        assert_eq!((l.left, l.width), (223.0, 736.0));
        // Too narrow for the card.
        assert!(!lane(1000.0, 848.0, false).card);
    }
}
