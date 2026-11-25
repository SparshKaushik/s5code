//! Web `useSettingsRestore`: Restore device defaults lists what it will reset, then asks first.

use super::{client, scope, update, value};
use crate::{App, Appearance};
use serde_json::{Value, json};

/// Web's `changedSettingLabels` after the theme, in order: each label and the settings it resets.
const ROWS: &[(&str, &[&str])] = &[
    ("Contrast", &["appearanceContrast"]),
    ("Glass opacity", &["glassOpacity"]),
    ("Diff colors", &["diffColorScheme"]),
    ("Chat width", &["chatWidth"]),
    ("Effort display", &["effortDisplay"]),
    ("Render math", &["mathRenderingEnabled"]),
    ("Panel animations", &["panelAnimationDurationMs"]),
    ("Environment identification", &["environmentIdentificationMode"]),
    ("Time format", &["timestampFormat"]),
    ("Thread notifications", &["notificationMode"]),
    ("In-app notifications", &["inAppNotificationsEnabled"]),
    ("Project Grouping", &["sidebarProjectGroupingMode"]),
    ("Project order", &["sidebarProjectSortOrder"]),
    ("Working section", &["sidebarWorkingShelfEnabled"]),
    ("Auto-settle inactive threads", &["sidebarAutoSettleAfterDays"]),
    ("Keep threads with a PR", &["sidebarAutoSettleKeepPullRequests"]),
    ("Auto-settle merged threads", &["sidebarAutoSettleOnMerge"]),
    ("Resume after usage limits", &["autoResumeLimitedThreads"]),
    ("Snooze until limits reset", &["snoozeLimitedThreads"]),
    ("Snooze options", &["snoozePresets"]),
    ("Word wrap", &["wordWrap"]),
    ("Composer context", &["persistComposerContextStrip"]),
    ("Interface font", &["fontFamilySans", "fontSizeInterface"]),
    ("Prompt font", &["fontFamilyComposer", "fontSizePrompt"]),
    ("Code font", &["fontFamilyCode", "fontSizeCode"]),
    ("Terminal font", &["fontFamilyTerminal", "fontSizeTerminal"]),
    ("Default diff file state", &["diffFilesCollapsed"]),
    ("Diff whitespace changes", &["diffIgnoreWhitespace"]),
    ("Diff layout", &["diffLayout"]),
    ("Open pull requests and changes panels proactively", &["proactivePanelsEnabled"]),
    ("Show skills in slash menu", &["showSkillsInSlashMenu"]),
    ("Rich text composer", &["composerRichTextEnabled"]),
    ("Send shortcut", &["sendShortcut"]),
    ("Follow-up behavior", &["followUpBehavior"]),
    ("Context window indicator", &["contextWindowMeterEnabled"]),
    ("Response streaming", &["responseStreamingMode"]),
    ("Provider update checks", &["enableProviderUpdateChecks"]),
    ("Continue threads after restarts", &["continueThreadsAfterServerUpdate"]),
    (
        "Background activity",
        &["backgroundActivity", "backgroundActivityProfile", "automaticGitFetchInterval", "providerHealthRefreshInterval"],
    ),
    ("New thread mode", &["defaultThreadEnvMode"]),
    ("New worktrees start from remote", &["newWorktreesStartFromOrigin"]),
    ("Add project base directory", &["addProjectBaseDirectory"]),
    ("Unpin confirmation", &["confirmThreadUnpin"]),
    ("Archive confirmation", &["confirmThreadArchive"]),
    ("Delete confirmation", &["confirmThreadDelete"]),
    ("Quit shortcut", &["confirmQuit"]),
    ("Text generation model", &["textGenerationModelSelection"]),
    ("Thread title instructions", &["threadTitleInstructions"]),
    ("Browser viewport", &["browserDefaultViewport"]),
    ("Browser zoom", &["browserDefaultZoomFactor"]),
    ("Browser appearance", &["browserDefaultAppearance"]),
    ("Recording frame rate", &["browserRecordingFrameRate"]),
    ("Recording key presses", &["browserRecordingShowKeyPresses"]),
    ("Recording mouse presses", &["browserRecordingShowMousePresses"]),
    ("Open links in", &["browserLinkTarget"]),
    ("Search engine", &["browserSearchEngine", "browserCustomSearchUrl"]),
    ("Floating preview", &["browserAutoShowFloatingPreview"]),
    ("Open pull requests in T3 Code", &["pullRequestLinksOpenInApp"]),
    ("Agent browser access", &["enableAgentBrowserAccess"]),
];

const THEME_DEFAULT: &str = "t3-code";

fn default_of(key: &str) -> Value {
    client::defaults().get(key).cloned().unwrap_or_else(|| scope::defaults()[key].clone())
}

/// The value a restore would replace, or None when the setting isn't known here (a server
/// that hasn't sent its settings yet changes nothing).
fn current(s: &App, key: &str) -> Option<Value> {
    if client::defaults().contains_key(key) || super::server_settings(s).get(key).is_some() {
        Some(value(s, key))
    } else {
        None
    }
}

fn dirty(s: &App, key: &str) -> bool {
    current(s, key).is_some_and(|v| v != default_of(key))
}

/// Rune's theme is `appearance` plus a theme per half: web's base theme and its mix.
fn theme_labels(appearance: &str, light: &str, dark: &str) -> Vec<&'static str> {
    let mut out = Vec::new();
    if appearance != "system" || (light == dark && light != THEME_DEFAULT) {
        out.push("Theme");
    }
    if light != dark {
        out.push("Theme mix");
    }
    out
}

fn theme_values(s: &App) -> (String, String, String) {
    let get = |key: &str, default: &str| value(s, key).as_str().unwrap_or(default).to_owned();
    (get("appearance", "system"), get("themeLight", THEME_DEFAULT), get("themeDark", THEME_DEFAULT))
}

/// Web `changedSettingLabels`.
pub fn changed_labels(s: &App) -> Vec<&'static str> {
    let (appearance, light, dark) = theme_values(s);
    let mut out = theme_labels(&appearance, &light, &dark);
    out.extend(ROWS.iter().filter(|(_, keys)| keys.iter().any(|k| dirty(s, k))).map(|(label, _)| *label));
    out
}

/// Web `"This will reset: …."`.
pub fn message(labels: &[&str]) -> String {
    format!("This will reset: {}.", labels.join(", "))
}

fn restore(s: &mut App, tx: &mut rune::Tx) {
    if dirty(s, super::fonts::ZOOM_KEY) {
        // Restoring the interface size zooms the window; land it at once.
        tx.instant();
    }
    let (appearance, light, dark) = theme_values(s);
    let theme_reset = appearance != "system" || light != THEME_DEFAULT || dark != THEME_DEFAULT;
    if appearance != "system" {
        update(s, "appearance", json!("system"));
        s.appearance = Appearance::System;
    }
    for (key, current) in [("themeLight", light), ("themeDark", dark)] {
        if current != THEME_DEFAULT {
            update(s, key, json!(THEME_DEFAULT));
        }
    }
    if theme_reset && s.client_settings.error.is_some() {
        crate::shell::palette::toast_error(s, "Couldn\u{2019}t restore theme settings", Some("Try again."));
    }
    let keys: Vec<&str> = ROWS.iter().flat_map(|(_, keys)| keys.iter().copied()).filter(|k| dirty(s, k)).collect();
    for key in keys {
        update(s, key, default_of(key));
    }
}

/// `restoreDefaults`: web's destructive `dialogs.confirm`, only with something to reset.
pub fn ask(s: &mut App) {
    let labels = changed_labels(s);
    if labels.is_empty() {
        return;
    }
    let question = format!("Restore default settings?\n{}", message(&labels));
    crate::shell::confirm::request_with(s, question, true, restore, |_: &mut App| {});
}

pub fn scene(s: &mut App) {
    s.client_settings.values.insert("chatWidth".into(), json!("wide"));
    s.client_settings.values.insert("diffLayout".into(), json!("split"));
    s.client_settings.values.insert("appearance".into(), json!("dark"));
    ask(s);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn restore_lists_what_it_resets_like_web() {
        assert!(theme_labels("system", THEME_DEFAULT, THEME_DEFAULT).is_empty());
        assert_eq!(theme_labels("dark", THEME_DEFAULT, THEME_DEFAULT), ["Theme"]);
        assert_eq!(theme_labels("system", "nord", "nord"), ["Theme"]);
        assert_eq!(theme_labels("system", THEME_DEFAULT, "nord"), ["Theme mix"]);
        assert_eq!(message(&["Theme", "Chat width"]), "This will reset: Theme, Chat width.");
        let mut s = App::new();
        s.client_settings = client::ClientSettings::default();
        let before = changed_labels(&s);
        assert!(!before.contains(&"Chat width"));
        s.client_settings.values.insert("chatWidth".into(), json!("wide"));
        s.client_settings.values.insert("fontSizeCode".into(), json!(15));
        let after = changed_labels(&s);
        assert!(after.contains(&"Chat width") && after.contains(&"Code font"));
        assert_eq!(after.len(), before.len() + 2);
    }
}
