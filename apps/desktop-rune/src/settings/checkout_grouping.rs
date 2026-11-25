//! Web `CheckoutGroupingSelect`: one checkout's sidebar grouping rule. Overrides are client
//! settings (`sidebarProjectGroupingOverrides`), so they stay editable without project write
//! access.

use rune::El;
use serde_json::{Value, json};

use crate::App;
use crate::kit;
use crate::theme::T;

const MODES: [&str; 4] = ["inherit", "repository", "repository_path", "separate"];
const KEY: &str = "sidebarProjectGroupingOverrides";

/// `PROJECT_GROUPING_MODE_LABELS`, with the list's name for "inherit".
fn label(mode: &str) -> &'static str {
    match mode {
        "repository" => "Group by repository",
        "repository_path" => "Group by repository path",
        "separate" => "Keep separate",
        _ => "Use global default",
    }
}

/// `withProjectGroupingOverride`: "inherit" drops the checkout's rule.
pub fn with_override(overrides: &Value, key: &str, selection: &str) -> Value {
    let mut next = overrides.as_object().cloned().unwrap_or_default();
    if selection == "inherit" {
        next.remove(key);
    } else {
        next.insert(key.to_owned(), json!(selection));
    }
    Value::Object(next)
}

fn select(s: &App, project: &Value) -> kit::Select {
    let key = crate::shell::sidebar::folders::physical_key(s, project);
    let selection = crate::settings::value(s, KEY)[&key].as_str().filter(|m| MODES.contains(m)).unwrap_or("inherit").to_owned();
    let global = crate::settings::value(s, "sidebarProjectGroupingMode");
    let display = if selection == "inherit" { format!("Default ({})", label(global.as_str().unwrap_or("repository"))) } else { label(&selection).to_owned() };
    let mut select = kit::Select::new("checkout-grouping", &MODES)
        .label(format!("Grouping rule for {}", crate::data::s(project, "workspaceRoot")))
        .width(220.0)
        .size("sm")
        .current(Some(&selection))
        .display(display)
        .open(s.ui.settings.checkout_grouping_open)
        .on_toggle(|s: &mut App| s.ui.settings.checkout_grouping_open ^= true)
        .on_close(|s: &mut App| s.ui.settings.checkout_grouping_open = false)
        .on_choose(move |s: &mut App, value: &str, _: &mut rune::Tx| {
            s.ui.settings.checkout_grouping_open = false;
            let next = with_override(&crate::settings::value(s, KEY), &key, value);
            crate::settings::update(s, KEY, next);
            if let Some(error) = s.client_settings.error.clone() {
                crate::shell::palette::toast_error(s, "Could not save grouping rule", Some(&error));
            }
        });
    for mode in MODES {
        select = select.option_name(mode, label(mode));
    }
    select
}

/// The trigger for the checkout row; its list mounts through `overlays`.
pub fn trigger(s: &App, t: &T, project: &Value) -> El {
    select(s, project).open(false).build(&kit::theme(t), &mut Vec::new())
}

pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    if !s.ui.settings.checkout_grouping_open {
        return;
    }
    let Some(project) = s.ui.settings.project.as_ref().and_then(|id| s.state().projects.iter().find(|p| crate::data::s(p, "id") == id)) else { return };
    select(s, project).build(&kit::theme(t), layers);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_checkout_rule_is_set_replaced_and_dropped_like_the_web() {
        let set = with_override(&json!({"other": "separate"}), "env:/a", "repository_path");
        assert_eq!(set, json!({"other": "separate", "env:/a": "repository_path"}));
        assert_eq!(with_override(&set, "env:/a", "inherit"), json!({"other": "separate"}), "FAILED: inherit drops the rule");
        assert_eq!(with_override(&Value::Null, "env:/a", "separate"), json!({"env:/a": "separate"}));
    }

    #[test]
    fn choosing_keep_separate_splits_the_checkout_into_its_own_sidebar_group() {
        let mut s = App::new();
        let project = s.state().projects[0].clone();
        let key = crate::shell::sidebar::folders::physical_key(&s, &project);
        crate::settings::update(&mut s, KEY, with_override(&Value::Null, &key, "separate"));
        assert_eq!(crate::settings::value(&s, KEY)[&key], "separate", "FAILED: the rule is a client setting");
        let grouping = crate::shell::sidebar::folders::Grouping::read(&s);
        assert!(grouping.is_scratch(&project) || grouping.key(&project) == key, "FAILED: a separate checkout groups by its physical key");
    }
}
