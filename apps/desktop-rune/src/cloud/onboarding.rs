//! Web `ConnectOnboardingDialog`: after a fresh sign-in, offers to publish this environment, then
//! to connect the account's other computers.
//!
//! Publishing belongs to the environment link controller. It reports this environment's link
//! state with [`set_link_state`], applies what Continue asks for from [`take_publish`], and answers
//! with [`published`]. Until it reports a state, the dialog shows only Connect devices, as web does
//! when there is no link target.

use rune::{El, col, row, text};

use super::{muted, shared};
use crate::theme::{self, AlphaMul, T};
use crate::{App, kit};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Step {
    Publish,
    Devices,
}

/// This environment's link (web `linkState.data`): whose account it is linked to, if any, and
/// what it publishes.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LinkState {
    pub linked_user: Option<String>,
    pub managed_tunnel: bool,
    pub publish_activity: bool,
}

#[derive(Debug, PartialEq)]
pub struct Onboarding {
    account: String,
    pub step: Step,
    pub expose: bool,
    pub publish: bool,
    pub dont_show: bool,
    prefilled: bool,
    /// Continue's selection, until the link controller takes it.
    requested: Option<(bool, bool)>,
    pub applying: bool,
    pub error: Option<String>,
}

/// Opens for `account` unless it opted out on this device.
pub(super) fn open(s: &mut App, account: &str) {
    if account.is_empty() || shared().opt_out.iter().any(|a| a == account) {
        return;
    }
    let step = if s.ui.cloud.link.is_some() { Step::Publish } else { Step::Devices };
    s.ui.cloud.onboarding = Some(Onboarding { account: account.to_owned(), step, expose: true, publish: true, dont_show: false, prefilled: false, requested: None, applying: false, error: None });
    prefill(s);
}

/// Web's one-time prefill: an environment already linked to this account keeps its choices.
fn prefill(s: &mut App) {
    let link = s.ui.cloud.link.clone();
    let Some(ui) = s.ui.cloud.onboarding.as_mut().filter(|o| !o.prefilled) else { return };
    let Some(link) = link else { return };
    ui.prefilled = true;
    if link.linked_user.as_deref() == Some(ui.account.as_str()) {
        ui.expose = link.managed_tunnel;
        ui.publish = link.publish_activity;
    }
}

pub(crate) fn set_link_state(s: &mut App, link: Option<LinkState>) {
    // Opened before the link state was known: Publish comes first once it is (web waits instead).
    if let Some(ui) = s.ui.cloud.onboarding.as_mut().filter(|o| link.is_some() && !o.prefilled && !o.applying) {
        ui.step = Step::Publish;
    }
    s.ui.cloud.link = link;
    if s.ui.cloud.link.is_none() {
        if let Some(ui) = s.ui.cloud.onboarding.as_mut().filter(|o| o.step == Step::Publish && !o.applying) {
            ui.step = Step::Devices;
        }
    }
    prefill(s);
}

/// Continue's selection for the link controller: a managed tunnel, and publishing activity.
pub(crate) fn take_publish(s: &mut App) -> Option<(bool, bool)> {
    s.ui.cloud.onboarding.as_mut()?.requested.take()
}

pub(crate) fn published(s: &mut App, result: Result<(), String>) {
    let Some(ui) = s.ui.cloud.onboarding.as_mut().filter(|o| o.applying) else { return };
    ui.applying = false;
    match result {
        Ok(()) => {
            ui.error = None;
            ui.step = Step::Devices;
            let description = if ui.expose { "This environment is available to your other devices through T3 Connect." } else { "This environment publishes agent activity to your mobile clients." };
            crate::shell::palette::toast(s, "T3 Connect enabled", Some(description));
        }
        Err(message) => ui.error = Some(message),
    }
}

fn apply(s: &mut App) {
    let can = s.ui.cloud.link.is_some();
    let Some(ui) = s.ui.cloud.onboarding.as_mut().filter(|o| !o.applying) else { return };
    // Onboarding only turns links on: with both off, existing links stay as they are.
    if !can || (!ui.expose && !ui.publish) {
        ui.step = Step::Devices;
        return;
    }
    ui.applying = true;
    ui.error = None;
    ui.requested = Some((ui.expose, ui.publish));
}

fn complete(s: &mut App) {
    let Some(ui) = s.ui.cloud.onboarding.take_if(|o| !o.applying) else { return };
    if ui.dont_show {
        let mut state = shared();
        if !state.opt_out.contains(&ui.account) {
            state.opt_out.push(ui.account);
            super::save(&state);
        }
    }
}

fn edit(s: &mut App, change: impl FnOnce(&mut Onboarding)) {
    if let Some(ui) = s.ui.cloud.onboarding.as_mut() {
        change(ui);
    }
}

fn toggle_row(key: &str, title: &str, description: &str, on: bool, disabled: bool, flip: fn(&mut Onboarding), t: &T) -> El {
    row()
        .key(format!("t3-connect-onboarding-{key}"))
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(16.0)
        .items_start()
        .child(col().grow(1.0).min_w(0.0).gap(4.0).child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground)).child(muted(description.to_owned(), t)))
        .child(kit::Switch::new(on).label(title.to_owned()).hide_label().disabled(disabled).on_toggle(move |s: &mut App| edit(s, flip)).build(&kit::theme(t)))
}

fn publish_step(ui: &Onboarding, t: &T) -> El {
    let disabled = ui.applying;
    let mut body = col().w_full().gap(12.0).child(
        col()
            .w_full()
            .radius(8.0)
            .border(1.0, t.c.border)
            .child(toggle_row("expose", "Publish this environment", "Make this environment available to your other devices through T3 Connect.", ui.expose, disabled, |o| o.expose = !o.expose, t))
            .child(rune::el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6)))
            .child(toggle_row(
                "publish",
                "Publish agent activity",
                "Send activity from this environment to your mobile clients for push notifications and Live Activities.",
                ui.publish,
                disabled,
                |o| o.publish = !o.publish,
                t,
            )),
    );
    if let Some(error) = &ui.error {
        body = body.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
    }
    body
}

fn devices_step(s: &App, t: &T) -> El {
    let empty = "No other environments are published to your account yet. Publish one from another device and it will show up here.";
    let mut list = col().w_full().radius(8.0).border(1.0, t.c.border);
    for (i, entry) in super::environment_rows(s, t, Some(empty)).into_iter().enumerate() {
        if i > 0 {
            list = list.child(rune::el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6)));
        }
        list = list.child(entry);
    }
    list
}

/// Web `WizardSteps`: numbered chips, each pressable while nothing is applying.
fn steps(ui: &Onboarding, t: &T) -> El {
    let current = if ui.step == Step::Publish { 0 } else { 1 };
    let mut chips = row().gap(6.0).center_y();
    for (i, (name, step)) in [("Publish", Step::Publish), ("Connect devices", Step::Devices)].into_iter().enumerate() {
        if i > 0 {
            chips = chips.child(rune::el().w(16.0).h(1.0).fill(t.c.border));
        }
        let lit = i <= current;
        let mut chip = row()
            .gap(6.0)
            .center_y()
            .child(row().size(18.0, 18.0).center().radius(9.0).fill(if lit { t.c.primary } else { t.c.muted }).child(text((i + 1).to_string()).font(theme::XS_MEDIUM).color(if lit { t.c.primary_foreground } else { t.c.muted_foreground })))
            .child(text(name).font(theme::XS_MEDIUM).color(if i == current { t.c.foreground } else { t.c.muted_foreground }));
        if i != current && !ui.applying {
            chip = chip.role("button").label(name).cursor(rune::Cursor::Pointer).on_press(move |s: &mut App| edit(s, |o| o.step = step));
        }
        chips = chips.child(chip);
    }
    chips
}

pub fn overlay(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(ui) = &s.ui.cloud.onboarding else { return };
    let publish = ui.step == Step::Publish && s.ui.cloud.link.is_some();
    let content = if publish { publish_step(ui, t) } else { devices_step(s, t) };
    let dont_show = ui.dont_show;
    let buttons = if publish {
        row()
            .gap(8.0)
            .child(kit::Button::new("Not now").variant("ghost").disabled(ui.applying).on_press(|s: &mut App| edit(s, |o| o.step = Step::Devices)).build(t))
            .child(kit::Button::new(if ui.applying { "Enabling…" } else { "Continue" }).disabled(ui.applying).on_press(apply).build(t))
    } else {
        row().child(kit::Button::new("Done").disabled(ui.applying).on_press(complete).build(t))
    };
    let footer = row()
        .w_full()
        .gap(12.0)
        .center_y()
        .child(row().grow(1.0).child(kit::Checkbox::new(dont_show).label("Don't show this again").on_toggle(move |s: &mut App| edit(s, |o| o.dont_show = !dont_show)).build(&kit::theme(t))))
        .child(buttons);
    let mut dialog = kit::Dialog::new("t3-connect-onboarding", "Set up T3 Connect")
        .open(true)
        .width(576.0)
        .description("Mesh your devices together. Publish this environment and connect the rest, all in one place.")
        .on_close(complete);
    if s.ui.cloud.link.is_some() {
        dialog = dialog.child(steps(ui, t));
    }
    dialog.child(content).child(footer).build(t, layers);
}

/// Scenes: just signed in, on Publish (this environment not linked yet) or on Connect devices.
pub(crate) fn scene(s: &mut App, publish: bool) {
    super::scene_signed_in(s);
    if publish {
        s.ui.cloud.link = Some(LinkState::default());
    }
    open(s, "user_1");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opened(link: Option<LinkState>) -> App {
        let mut s = App::new();
        s.ui.cloud.link = link;
        open(&mut s, "user_1");
        s
    }

    #[test]
    fn publish_shows_only_with_a_link_target_and_prefills_from_this_account_only() {
        assert_eq!(opened(None).ui.cloud.onboarding.unwrap().step, Step::Devices);
        let mine = opened(Some(LinkState { linked_user: Some("user_1".into()), managed_tunnel: false, publish_activity: true })).ui.cloud.onboarding.unwrap();
        assert_eq!((mine.step, mine.expose, mine.publish), (Step::Publish, false, true));
        let theirs = opened(Some(LinkState { linked_user: Some("user_2".into()), managed_tunnel: false, publish_activity: false })).ui.cloud.onboarding.unwrap();
        assert_eq!((theirs.expose, theirs.publish), (true, true), "another account's link does not prefill");
    }

    #[test]
    fn continue_hands_the_selection_over_and_lands_on_devices_once_applied() {
        let mut s = opened(Some(LinkState::default()));
        edit(&mut s, |o| o.publish = false);
        apply(&mut s);
        assert_eq!(take_publish(&mut s), Some((true, false)));
        assert_eq!(take_publish(&mut s), None, "taken once");
        complete(&mut s);
        assert!(s.ui.cloud.onboarding.is_some(), "it stays open while applying");
        published(&mut s, Err("Relay unavailable".into()));
        let ui = s.ui.cloud.onboarding.as_ref().unwrap();
        assert_eq!((ui.step, ui.error.as_deref(), ui.applying), (Step::Publish, Some("Relay unavailable"), false));
        apply(&mut s);
        published(&mut s, Ok(()));
        assert_eq!(s.ui.cloud.onboarding.as_ref().unwrap().step, Step::Devices);
        assert_eq!(crate::shell::palette::toast_description(&s), Some("This environment is available to your other devices through T3 Connect."));
    }

    #[test]
    fn both_toggles_off_skips_to_devices_without_unlinking() {
        let mut s = opened(Some(LinkState::default()));
        edit(&mut s, |o| {
            o.expose = false;
            o.publish = false;
        });
        apply(&mut s);
        assert_eq!(take_publish(&mut s), None);
        assert_eq!(s.ui.cloud.onboarding.as_ref().unwrap().step, Step::Devices);
    }

    #[test]
    fn dont_show_again_keeps_it_closed_for_that_account() {
        // Its own accounts: the opt-out list is process-wide and other tests open user_1.
        let mut s = App::new();
        open(&mut s, "user_opted_out");
        edit(&mut s, |o| o.dont_show = true);
        complete(&mut s);
        assert!(s.ui.cloud.onboarding.is_none());
        open(&mut s, "user_opted_out");
        assert!(s.ui.cloud.onboarding.is_none(), "opted out on this device");
        open(&mut s, "user_not_opted_out");
        assert!(s.ui.cloud.onboarding.is_some());
    }
}
