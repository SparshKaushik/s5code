//! ThreadDetailsItemsMenu and threadDetailsItems.ts: which items the thread details card shows.
//! The choices live in the client settings Electron shares, so web reads the same ones.

use rune::El;
use serde_json::{Map, Value, json};

use crate::App;
use crate::kit::{MenuEntry, MenuItem, Placement};
use crate::theme::T;

pub struct Item {
    pub id: &'static str,
    pub label: &'static str,
    default_visible: bool,
}

const fn item(id: &'static str, label: &'static str, default_visible: bool) -> Item {
    Item { id, label, default_visible }
}

// Ids are stored in client settings, so never rename one.
pub const ITEMS: [Item; 12] = [
    item("environment", "Environment and worktree", true),
    item("openIn", "Open in editor", true),
    item("scripts", "Project scripts", true),
    item("branch", "Branch and pull request", true),
    item("gitActions", "Git actions", true),
    item("changes", "Changes", true),
    item("servers", "Servers", true),
    item("automations", "Automations", true),
    item("lineage", "Lineage", true),
    item("sideChats", "Side chats", true),
    item("usage", "Usage limits", false),
    item("context", "Context window", false),
];

const ITEMS_KEY: &str = "threadDetailsItems";
const USAGE_KEY: &str = "threadDetailsUsageEnabled";

/// Where the menu opens: at a right-click on the card, or under "Choose what to show".
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum At {
    Point(f32, f32),
    Choose,
}

pub const CHOOSE_KEY: &str = "details-choose-items";

// People who turned on the retired Labs toggle keep Usage and Context until they change them here.
fn default_visible(item: &Item, settings: &Map<String, Value>) -> bool {
    match item.id {
        "usage" | "context" => settings.get(USAGE_KEY).and_then(Value::as_bool).unwrap_or(false),
        _ => item.default_visible,
    }
}

/// resolveThreadDetailsVisibility, for one item.
pub fn visible(settings: &Map<String, Value>, id: &str) -> bool {
    let Some(item) = ITEMS.iter().find(|i| i.id == id) else { return false };
    settings.get(ITEMS_KEY).and_then(|v| v[id].as_bool()).unwrap_or_else(|| default_visible(item, settings))
}

/// setThreadDetailsItem: stores only choices that differ from the default, so a later default
/// change reaches everyone else.
fn with_item(settings: &Map<String, Value>, item: &Item, on: bool) -> Value {
    let mut rest = settings.get(ITEMS_KEY).and_then(Value::as_object).cloned().unwrap_or_default();
    rest.remove(item.id);
    if on != default_visible(item, settings) {
        rest.insert(item.id.into(), json!(on));
    }
    Value::Object(rest)
}

/// isDefaultThreadDetailsVisibility.
pub fn is_default(settings: &Map<String, Value>) -> bool {
    ITEMS.iter().all(|i| visible(settings, i.id) == i.default_visible)
}

fn set(s: &mut App, id: &str, on: bool) {
    let Some(item) = ITEMS.iter().find(|i| i.id == id) else { return };
    let next = with_item(&s.client_settings.values, item, on);
    s.client_settings.values.insert(ITEMS_KEY.into(), next);
    s.client_settings.save();
}

/// RESET_THREAD_DETAILS_ITEMS: also clears the retired Labs flag, so Usage and Context return to off.
pub fn reset(s: &mut App) {
    s.client_settings.values.insert(ITEMS_KEY.into(), json!({}));
    s.client_settings.values.insert(USAGE_KEY.into(), json!(false));
    s.client_settings.save();
}

/// ThreadDetailsItemsMenuItems: the card's context menu and Settings share it.
pub fn entries(s: &App) -> Vec<MenuEntry> {
    let settings = &s.client_settings.values;
    let mut out = vec![MenuEntry::Label("Show in thread details".into())];
    for item in &ITEMS {
        let (id, on) = (item.id, visible(settings, item.id));
        out.push(MenuEntry::item(MenuItem::new(item.label).checked(on).on_select(move |s: &mut App| set(s, id, !on))));
    }
    out.push(MenuEntry::Separator);
    out.push(MenuEntry::item(MenuItem::new("Reset to default").disabled(is_default(settings)).on_select(|s: &mut App| reset(s))));
    out
}

/// Settings' "{enabled} of {total} on" summary.
pub fn summary(s: &App) -> String {
    let on = ITEMS.iter().filter(|i| visible(&s.client_settings.values, i.id)).count();
    format!("{on} of {} on", ITEMS.len())
}

pub fn open(s: &mut App, at: At) {
    s.ui.chat.details.menu = None;
    s.ui.chat.details.items_menu = Some(at);
}

pub fn overlay(s: &App, t: &T, out: &mut Vec<El>) {
    let Some(at) = s.ui.chat.details.items_menu else { return };
    let menu = crate::kit::Menu::new("details-items").entries(entries(s)).open(true).min_width(220.0).on_close(|s: &mut App| s.ui.chat.details.items_menu = None);
    let menu = match at {
        At::Point(x, y) => menu.at(x, y),
        At::Choose => menu.anchor(CHOOSE_KEY).placement(Placement::BottomStart),
    };
    menu.overlay(&crate::kit::theme(t), out);
}


/// Scene: the menu opened by a right-click on the card, with two items turned off.
pub fn scene(s: &mut App) {
    super::scene_thread(s);
    s.client_settings.values.insert(ITEMS_KEY.into(), json!({"changes": false, "usage": true}));
    open(s, At::Point(1180.0, 140.0));
}

/// Scene: every item off leaves "Choose what to show", with its menu open under it.
pub fn scene_empty(s: &mut App) {
    super::scene_thread(s);
    let off: Map<String, Value> = ITEMS.iter().filter(|i| i.default_visible).map(|i| (i.id.to_string(), json!(false))).collect();
    s.client_settings.values.insert(ITEMS_KEY.into(), Value::Object(off));
    open(s, At::Choose);
}

#[cfg(test)]
mod tests {
    use super::{ITEMS, is_default, visible, with_item};
    use serde_json::{Map, Value, json};

    fn settings(v: Value) -> Map<String, Value> {
        v.as_object().cloned().unwrap()
    }

    #[test]
    fn defaults_show_all_but_usage_and_context_unless_labs_turned_them_on() {
        let empty = settings(json!({}));
        let shown: Vec<&str> = ITEMS.iter().filter(|i| visible(&empty, i.id)).map(|i| i.id).collect();
        assert_eq!(shown.len(), 10);
        assert!(!visible(&empty, "usage") && !visible(&empty, "context"));
        assert!(is_default(&empty));
        let labs = settings(json!({"threadDetailsUsageEnabled": true}));
        assert!(visible(&labs, "usage") && visible(&labs, "context"));
        assert!(!is_default(&labs), "the retired flag still differs from the defaults Reset restores");
        assert!(!visible(&empty, "unknown"));
    }

    #[test]
    fn only_choices_that_differ_from_the_default_are_stored() {
        let item = |id: &str| ITEMS.iter().find(|i| i.id == id).unwrap();
        let empty = settings(json!({}));
        assert_eq!(with_item(&empty, item("servers"), false), json!({"servers": false}));
        let hidden = settings(json!({"threadDetailsItems": {"servers": false, "branch": false}}));
        assert!(!visible(&hidden, "servers") && !is_default(&hidden));
        assert_eq!(with_item(&hidden, item("servers"), true), json!({"branch": false}));
        // Usage defaults on for a Labs user, so turning it on stores nothing for them.
        let labs = settings(json!({"threadDetailsUsageEnabled": true}));
        assert_eq!(with_item(&labs, item("usage"), true), json!({}));
        assert_eq!(with_item(&empty, item("usage"), true), json!({"usage": true}));
    }
}
