//! Web `ThreadDetailsItemsRow`: which items the thread details card shows.

use super::{update, value};
use crate::kit::{self, Menu, MenuItem, Placement};
use crate::theme::T;
use crate::{App, icons};
use rune::{El, row};
use serde_json::{Map, Value, json};

/// Web `THREAD_DETAILS_ITEMS`: id, label, default. Ids are stored in client settings.
pub const ITEMS: &[(&str, &str, bool)] = &[
    ("environment", "Environment and worktree", true),
    ("openIn", "Open in editor", true),
    ("scripts", "Project scripts", true),
    ("branch", "Branch and pull request", true),
    ("gitActions", "Git actions", true),
    ("changes", "Changes", true),
    ("servers", "Servers", true),
    ("automations", "Automations", true),
    ("lineage", "Lineage", true),
    ("sideChats", "Side chats", true),
    ("usage", "Usage limits", false),
    ("context", "Context window", false),
];

/// People who turned on the retired Labs toggle keep Usage and Context until they change them here.
fn default_visible(id: &str, default: bool, usage_enabled: bool) -> bool {
    if matches!(id, "usage" | "context") { usage_enabled } else { default }
}

/// Web `resolveThreadDetailsVisibility`, in `ITEMS` order.
pub fn visibility(items: &Value, usage_enabled: bool) -> Vec<bool> {
    ITEMS.iter().map(|(id, _, default)| items[*id].as_bool().unwrap_or_else(|| default_visible(id, *default, usage_enabled))).collect()
}

/// Web `setThreadDetailsItem`: stores only choices that differ from the default.
pub fn set(items: &Value, usage_enabled: bool, id: &str, visible: bool) -> Value {
    let mut next: Map<String, Value> = items.as_object().cloned().unwrap_or_default();
    next.remove(id);
    let default = ITEMS.iter().find(|(i, ..)| *i == id).is_some_and(|(_, _, d)| default_visible(id, *d, usage_enabled));
    if visible != default {
        next.insert(id.to_owned(), json!(visible));
    }
    Value::Object(next)
}

/// Web `isDefaultThreadDetailsVisibility`: against each item's own default, not the Labs one.
pub fn is_default(visibility: &[bool]) -> bool {
    ITEMS.iter().zip(visibility).all(|((.., default), on)| on == default)
}

fn current(s: &App) -> Vec<bool> {
    visibility(&value(s, "threadDetailsItems"), value(s, "threadDetailsUsageEnabled") == true)
}

/// Web `RESET_THREAD_DETAILS_ITEMS`: also clears the retired Labs flag.
pub fn reset(s: &mut App) {
    update(s, "threadDetailsItems", json!({}));
    update(s, "threadDetailsUsageEnabled", json!(false));
}

pub fn customized(s: &App) -> bool {
    !is_default(&current(s))
}

pub fn control(s: &App, t: &T) -> El {
    let on = current(s).into_iter().filter(|v| *v).count();
    let open = s.ui.settings.thread_details_open;
    row().key("thread-details-trigger").child(
        kit::Button::new(format!("{on} of {} on", ITEMS.len()))
            .variant("outline")
            .size("sm")
            .on_press(move |s: &mut App| s.ui.settings.thread_details_open = !open)
            .build(t)
            .w(160.0)
            .label(format!("Thread details: {on} of {} on", ITEMS.len()))
            .child(rune::icon(&icons::CHEVRON_DOWN, 14.0).color(t.c.muted_foreground))
            .expanded(open),
    )
}

/// Web `ThreadDetailsItemsMenuItems`.
pub fn overlay(s: &App, t: &T, layers: &mut Vec<El>) {
    if !s.ui.settings.thread_details_open {
        return;
    }
    let shown = current(s);
    let mut menu = Menu::new("thread-details-menu")
        .anchor("thread-details-trigger")
        .placement(Placement::BottomEnd)
        .gutter(4.0)
        .width(232.0)
        .open(true)
        .on_close(|s: &mut App| s.ui.settings.thread_details_open = false)
        .group_label("Show in thread details");
    for (i, (id, label, _)) in ITEMS.iter().enumerate() {
        let next = !shown[i];
        menu = menu.item(MenuItem::new(*label).checked(shown[i]).on_select(move |s: &mut App| {
            let items = set(&value(s, "threadDetailsItems"), value(s, "threadDetailsUsageEnabled") == true, id, next);
            update(s, "threadDetailsItems", items);
        }));
    }
    menu = menu.separator().item(MenuItem::new("Reset to default").disabled(is_default(&shown)).on_select(reset));
    menu.overlay(&kit::theme(t), layers);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn thread_details_store_only_choices_off_their_default_like_web() {
        let none = json!({});
        let shown = visibility(&none, false);
        assert!(is_default(&shown));
        assert_eq!(shown.iter().filter(|v| **v).count(), 10, "Usage and Context start off");
        // The retired Labs toggle keeps Usage and Context on, which is not the default.
        assert!(!is_default(&visibility(&none, true)));
        let hidden = set(&none, false, "servers", false);
        assert_eq!(hidden, json!({"servers": false}));
        assert_eq!(set(&hidden, false, "servers", true), json!({}), "back to its default, the choice is dropped");
        assert_eq!(set(&none, true, "usage", true), json!({}), "on by the Labs flag already");
        assert_eq!(set(&none, false, "usage", true), json!({"usage": true}));
    }
}
