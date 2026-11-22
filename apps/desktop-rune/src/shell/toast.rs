//! The app's toast stack (web `components/ui/toast.tsx` over Base UI's toast manager): top
//! right under the topbar, the newest three shown, older ones peeking behind the front one until
//! the stack is hovered. Timers pause while the stack is hovered or the window is in the
//! background, and a toast tied to a thread shows only while that thread is open.
use std::rc::Rc;

use rune::{Cursor, Cx, El, Tx, col, el, icon, row, text};

use crate::theme::{self, AlphaMul, T, layout};
use crate::{App, Route, icons, kit};

/// Base UI's default timeout and limit.
pub const TIMEOUT_MS: i64 = 5000;
const LIMIT: usize = 3;
const WIDTH: f32 = 360.0;
const GAP: f32 = 12.0;
const PEEK: f32 = 12.0;
/// Errors this long clamp to four lines (`ERROR_DESCRIPTION_CLAMP_MIN_CHARS`).
const CLAMP_MIN_CHARS: usize = 180;
pub(crate) const COPY_ERROR: &str = "Copy error";

/// A button: its label, whether it is the primary one, and what it does.
pub(crate) type Action = (String, bool, Rc<dyn Fn(&mut App, &mut Tx)>);

/// `TOAST_ICONS` by toast `type`; `Plain` has none.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub enum Kind {
    #[default]
    Plain,
    Success,
    Info,
    Loading,
    Warning,
    Error,
}

/// A custom leading icon (`data.leadingIcon`) and its colour.
#[derive(Clone, Copy)]
pub(crate) enum Tone {
    Success,
    Warning,
    Destructive,
    Info,
}

pub(crate) struct Toast {
    pub id: u64,
    pub title: String,
    pub description: Option<String>,
    pub kind: Kind,
    pub lead: Option<(&'static rune::Icon, Tone)>,
    pub actions: Vec<Action>,
    /// Actions shown disabled, by label.
    pub disabled: Vec<String>,
    /// Actions reading "Copied" until their deadline.
    pub copied: Vec<(String, i64)>,
    /// When it closes; `i64::MAX` for `timeout: 0`.
    pub until: i64,
    /// `expandableContent` behind "Show details" / "Hide details".
    pub details: Option<String>,
    pub details_open: bool,
    /// `data.threadId`: shown only while this thread is open.
    pub thread: Option<String>,
    /// `hideCopyButton`: an error toast copies its description unless this is set.
    pub hide_copy: bool,
}

impl Toast {
    pub(crate) fn new(title: impl Into<String>, description: Option<&str>, kind: Kind) -> Toast {
        Toast {
            id: 0,
            title: title.into(),
            description: description.filter(|d| !d.is_empty()).map(str::to_owned),
            kind,
            lead: None,
            actions: vec![],
            disabled: vec![],
            copied: vec![],
            until: 0,
            details: None,
            details_open: false,
            thread: None,
            hide_copy: false,
        }
    }
}

#[derive(Default)]
pub struct Stack {
    toasts: Vec<Toast>,
    next: u64,
    hovered: bool,
    /// Since when the timers have been held (hover, or the window in the background).
    paused_at: Option<i64>,
}

/// Adds `toast`, closing after `timeout` ms (`None` stays until dismissed); returns its id.
pub(crate) fn add(s: &mut App, mut toast: Toast, timeout: Option<i64>) -> u64 {
    let now = crate::data::now_ms();
    let stack = &mut s.ui.shell.toasts;
    stack.next += 1;
    toast.id = stack.next;
    // A held stack starts the new timer when it resumes.
    let start = stack.paused_at.unwrap_or(now);
    toast.until = timeout.map_or(i64::MAX, |ms| start + ms);
    stack.toasts.push(toast);
    stack.next
}

/// `toastManager.update`: replaces a toast still open in place, keeping its id. A toast the user
/// already closed stays closed.
pub(crate) fn update(s: &mut App, id: u64, mut toast: Toast, timeout: Option<i64>) {
    let now = crate::data::now_ms();
    let stack = &mut s.ui.shell.toasts;
    let start = stack.paused_at.unwrap_or(now);
    if let Some(slot) = stack.toasts.iter_mut().find(|t| t.id == id) {
        toast.id = id;
        toast.until = timeout.map_or(i64::MAX, |ms| start + ms);
        *slot = toast;
    }
}

pub(crate) fn newest(s: &App) -> Option<&Toast> {
    s.ui.shell.toasts.toasts.last()
}

pub(crate) fn newest_mut(s: &mut App) -> Option<&mut Toast> {
    s.ui.shell.toasts.toasts.last_mut()
}

pub(crate) fn close(s: &mut App, id: u64) {
    s.ui.shell.toasts.toasts.retain(|t| t.id != id);
}

/// Closes every toast titled `title`.
pub(crate) fn close_titled(s: &mut App, title: &str) {
    s.ui.shell.toasts.toasts.retain(|t| t.title != title);
}

/// The id the next toast will get, for an action that closes its own toast.
pub(crate) fn next_id(s: &App) -> u64 {
    s.ui.shell.toasts.next + 1
}

#[cfg(test)]
pub(crate) fn titles(s: &App) -> Vec<(String, Option<String>)> {
    s.ui.shell.toasts.toasts.iter().map(|t| (t.title.clone(), t.description.clone())).collect()
}

/// `shouldRenderThreadScopedToast`.
fn visible(s: &App, toast: &Toast) -> bool {
    match &toast.thread {
        None => true,
        Some(thread) => matches!(&s.route, Route::Thread(open) if open == thread),
    }
}

/// Holds the timers while the stack is hovered or the window is in the background, and shifts
/// their deadlines by the held time when it resumes.
fn sync_pause(s: &mut App, now: i64) {
    let hold = s.ui.shell.toasts.hovered || s.ui.notify.background;
    let stack = &mut s.ui.shell.toasts;
    match (hold, stack.paused_at) {
        (true, None) => stack.paused_at = Some(now),
        (false, Some(at)) => {
            let held = now - at;
            for toast in &mut stack.toasts {
                if toast.until != i64::MAX {
                    toast.until += held;
                }
            }
            stack.paused_at = None;
        }
        _ => {}
    }
}

/// Expires toasts and "Copied" labels, and wakes the loop for the next deadline.
pub fn poll(s: &mut App, tx: &mut Tx) -> bool {
    let now = crate::data::now_ms();
    sync_pause(s, now);
    let stack = &mut s.ui.shell.toasts;
    let before: usize = stack.toasts.len() + stack.toasts.iter().map(|t| t.copied.len()).sum::<usize>();
    for toast in &mut stack.toasts {
        toast.copied.retain(|(_, until)| now < *until);
    }
    if stack.paused_at.is_none() {
        stack.toasts.retain(|t| t.until > now);
    }
    let after: usize = stack.toasts.len() + stack.toasts.iter().map(|t| t.copied.len()).sum::<usize>();
    let copies = stack.toasts.iter().flat_map(|t| t.copied.iter().map(|(_, at)| *at));
    let timers = stack.toasts.iter().map(|t| t.until).filter(|_| stack.paused_at.is_none());
    if let Some(next) = copies.chain(timers).filter(|at| *at != i64::MAX).min() {
        tx.wake_after(std::time::Duration::from_millis((next - now).max(1) as u64));
    }
    before != after
}

fn set_hovered(s: &mut App, hovered: bool) {
    s.ui.shell.toasts.hovered = hovered;
    sync_pause(s, crate::data::now_ms());
}

fn kind_icon(t: &T, kind: Kind) -> Option<El> {
    let (glyph, color) = match kind {
        Kind::Plain => return None,
        Kind::Loading => return Some(kit::Spinner::new().size("sm").tone("muted").build(&kit::theme(t)).opacity(0.8)),
        Kind::Success => (&icons::CIRCLE_CHECK, t.c.success),
        Kind::Info => (&icons::INFO, t.c.info),
        Kind::Warning => (&icons::TRIANGLE_ALERT, t.c.warning),
        Kind::Error => (&icons::CIRCLE_ALERT, t.c.destructive),
    };
    Some(icon(glyph, 16.0).color(color))
}

fn tone(t: &T, tone: Tone) -> rune::Color {
    match tone {
        Tone::Success => t.c.success_foreground,
        Tone::Warning => t.c.warning_foreground,
        Tone::Destructive => t.c.destructive_foreground,
        Tone::Info => t.c.info_foreground,
    }
}

fn card(toast: &Toast, t: &T) -> El {
    let k = kit::theme(t);
    let id = toast.id;
    let lead = toast.lead.map(|(glyph, c)| icon(glyph, 16.0).color(tone(t, c))).or_else(|| kind_icon(t, toast.kind));
    let copy_error = toast.description.clone().filter(|_| toast.kind == Kind::Error && !toast.hide_copy);
    let trailing = copy_error.is_some() || !toast.actions.is_empty();
    let mut title = row().gap(8.0).min_w(0.0);
    if let Some(lead) = lead {
        title = title.child(el().w(16.0).h(20.0).center().shrink().child(lead));
    }
    title = title.child(text(toast.title.clone()).font(theme::SM_MEDIUM).color(t.c.popover_foreground).wrap().shrink().min_w(0.0));
    let mut body = col().grow(1.0).shrink().min_w(0.0).gap(2.0).child(title);
    if let Some(description) = &toast.description {
        let mut line = text(description.clone()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap();
        if toast.kind == Kind::Error && description.chars().count() >= CLAMP_MIN_CHARS {
            line = line.max_lines(4);
        }
        body = body.child(line);
    }
    if let Some(details) = &toast.details {
        let open = toast.details_open;
        let label = if open { "Hide details" } else { "Show details" };
        body = body.child(
            row()
                .key(format!("toast-{id}-details"))
                .role("button")
                .label(label)
                .gap(4.0)
                .py(2.0)
                .center_y()
                .cursor(Cursor::Pointer)
                .child(icon(if open { &icons::CHEVRON_UP } else { &icons::CHEVRON_DOWN }, 14.0).color(t.c.muted_foreground.alpha_mul(0.8)))
                .child(text(label).font(theme::XS_MEDIUM).color(t.c.muted_foreground))
                .on_press(move |s: &mut App| {
                    if let Some(toast) = s.ui.shell.toasts.toasts.iter_mut().find(|t| t.id == id) {
                        toast.details_open = !toast.details_open;
                    }
                }),
        );
        if open {
            body = body.child(
                col().key(format!("toast-{id}-detail-panel")).mt(8.0).max_h(160.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(text(details.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().selectable(true)),
            );
        }
    }
    let mut content = row().w_full().pl(14.0).pr(if trailing { 24.0 } else { 14.0 }).py(12.0).gap(6.0).center_y().child(body);
    if trailing {
        let mut controls = row().gap(6.0).center_y().shrink();
        if let Some(text) = copy_error {
            let copied = toast.copied.iter().any(|(l, _)| l == COPY_ERROR);
            let label = if copied { "Copied error" } else { COPY_ERROR };
            let button = kit::Button::new(label).icon(if copied { &icons::CHECK } else { &icons::COPY }).size("icon-micro").variant("ghost-muted").build(t).key(format!("toast-{id}-copy-error")).help(label);
            controls = controls.child(if copied {
                button
            } else {
                button.on_press_tx(move |s: &mut App, tx: &mut Tx| {
                    tx.copy(&text);
                    copied_on(s, tx, id, COPY_ERROR);
                })
            });
        }
        let (primary, rest): (Vec<_>, Vec<_>) = toast.actions.iter().partition(|(_, p, _)| *p);
        for (label, primary, run) in rest.into_iter().chain(primary) {
            let copied = toast.copied.iter().any(|(l, _)| l == label);
            let off = toast.disabled.contains(label);
            let button = kit::Button::new(if copied { "Copied!".to_string() } else { label.clone() })
                .variant(if *primary { "default" } else { "outline" })
                .size("xs")
                .disabled(copied || off)
                .build(t);
            let run = run.clone();
            controls = controls.child(if copied || off { button } else { button.on_press_tx(move |s: &mut App, tx: &mut Tx| run(s, tx)) });
        }
        content = content.child(controls);
    }
    let dismiss = el()
        .key(format!("toast-{id}-dismiss"))
        .role("button")
        .label("Dismiss notification")
        .pin(None, Some(-6.0), Some(-6.0), None)
        .size(24.0, 24.0)
        .center()
        .radius(theme::radius::FULL)
        .fill(t.c.popover.alpha_mul(0.92))
        .border(1.0, t.c.border.alpha_mul(0.6))
        .cursor(Cursor::Pointer)
        .child(icon(&icons::X, 12.0).color(t.c.muted_foreground))
        .on_press(move |s: &mut App| close(s, id));
    kit::motion::popup(kit::menu::glass(col().key(format!("toast-{id}")).role("status").w_full().child(content).child(dismiss), &k, false))
}

/// The toast `id`'s `label` action reads "Copied" for 2 s.
pub(crate) fn copied_on(s: &mut App, tx: &mut Tx, id: u64, label: &str) {
    let Some(toast) = s.ui.shell.toasts.toasts.iter_mut().find(|t| t.id == id) else { return };
    let until = crate::data::now_ms() + 2000;
    toast.copied.retain(|(l, _)| l != label);
    toast.copied.push((label.to_owned(), until));
    if toast.until != i64::MAX {
        toast.until = toast.until.max(until);
    }
    tx.wake_after(std::time::Duration::from_millis(2000));
}

/// The visible stack, newest first: the front toast and up to two peeking behind it, or all three
/// laid out while hovered.
pub fn view(s: &App, cx: &mut Cx<App>, t: &T) -> Option<El> {
    let stack = &s.ui.shell.toasts;
    let shown: Vec<&Toast> = stack.toasts.iter().rev().filter(|toast| visible(s, toast)).take(LIMIT).collect();
    let front = shown.first()?;
    let region = col()
        .key("toast-region")
        .role("region")
        .label("Notifications")
        .transition(rune::Transition::none())
        // Above modal popovers and dialogs, as Base UI's viewport sits above them on web.
        .raise()
        .pin(None, Some(32.0 + layout::TOPBAR_H), Some(32.0), None)
        .w(WIDTH)
        .on_hover_change(|s: &mut App, inside, _: &mut Tx| set_hovered(s, inside));
    if stack.hovered {
        return Some(region.gap(GAP).children(shown.iter().map(|toast| card(toast, t))));
    }
    // Collapsed: each older toast is the front one's height, scaled by 10% a step from the top
    // edge and peeking 12 px further below.
    let height = cx.layout_with("toast-front", |r| r[3]).unwrap_or(64.0);
    let k = kit::theme(t);
    let mut stacked = el().w_full();
    for (index, toast) in shown.iter().enumerate().skip(1).rev() {
        let i = index as f32;
        let scale = 1.0 - 0.1 * i;
        stacked = stacked.child(
            kit::menu::glass(el().key(format!("toast-{}", toast.id)).w(WIDTH * scale).h(height * scale), &k, false)
                .absolute(WIDTH * 0.05 * i, i * PEEK + 0.1 * i * height),
        );
    }
    Some(region.child(stacked.child(el().key("toast-front").w_full().child(card(front, t)))))
}

/// Render scenes: a success, an info and an error toast with details; `expanded` hovers the stack.
pub(crate) fn scene(s: &mut App, expanded: bool) {
    add(s, Toast::new("Project created", Some("s5code is ready in the sidebar."), Kind::Success), None);
    add(s, Toast::new("Process already exited", None, Kind::Info), None);
    let mut failed = Toast::new("Could not push", Some("The remote rejected the update because the branch moved."), Kind::Error);
    failed.details = Some("To github.com:o/r.git\n ! [rejected] main -> main (fetch first)\nerror: failed to push some refs".into());
    failed.details_open = expanded;
    add(s, failed, None);
    s.ui.shell.toasts.hovered = expanded;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timers_hold_while_hovered_or_in_the_background_and_resume_where_they_were() {
        let mut s = App::new();
        let now = crate::data::now_ms();
        let id = add(&mut s, Toast::new("Saved", None, Kind::Success), Some(TIMEOUT_MS));
        let until = newest(&s).unwrap().until;
        s.ui.shell.toasts.hovered = true;
        sync_pause(&mut s, now);
        s.ui.shell.toasts.hovered = false;
        sync_pause(&mut s, now + 3000);
        assert_eq!(newest(&s).unwrap().until, until + 3000, "the held time is added back");
        s.ui.notify.background = true;
        sync_pause(&mut s, now + 4000);
        // A toast raised while held starts its timer on resume.
        let held = add(&mut s, Toast::new("Later", None, Kind::Plain), Some(1000));
        assert_eq!(s.ui.shell.toasts.toasts.iter().find(|t| t.id == held).unwrap().until, now + 4000 + 1000);
        close(&mut s, id);
        assert_eq!(titles(&s), [("Later".to_owned(), None)]);
    }

    #[test]
    fn a_thread_toast_shows_only_on_its_thread_and_the_stack_shows_three() {
        let mut s = App::new();
        let mut scoped = Toast::new("Pushed", None, Kind::Success);
        scoped.thread = Some("t1".into());
        add(&mut s, scoped, None);
        assert!(!visible(&s, newest(&s).unwrap()));
        s.route = Route::Thread("t1".into());
        assert!(visible(&s, newest(&s).unwrap()));
        for n in 0..4 {
            add(&mut s, Toast::new(format!("n{n}"), None, Kind::Plain), None);
        }
        let shown: Vec<_> = s.ui.shell.toasts.toasts.iter().rev().filter(|t| visible(&s, t)).take(LIMIT).map(|t| t.title.clone()).collect();
        assert_eq!(shown, ["n3", "n2", "n1"]);
    }
}
