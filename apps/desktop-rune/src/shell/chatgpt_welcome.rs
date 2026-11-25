//! ChatGptWelcomeCoordinator: once per connected Codex profile billed to a shared ChatGPT plan, a
//! dialog says so and offers ChatGPT's usage page.

use std::cell::OnceCell;

use rune::{El, Tx, icon, row, text};
use serde_json::{Value, json};

use crate::theme::{self, T};
use crate::{App, data, env, kit};

/// CHATGPT_USAGE_URL.
const CHATGPT_USAGE_URL: &str = "https://chatgpt.com/#settings/Usage";
/// Web localStorage `t3:chatgpt-sharing-welcome:v1`.
const ACKNOWLEDGED_KEY: &str = "chatgptSharingWelcome";

#[derive(Default)]
pub struct Ui {
    /// Profiles already welcomed, read once.
    acknowledged: OnceCell<Vec<String>>,
}

fn prefs() -> rune::prefs::Prefs {
    if rune::is_headless() || cfg!(test) {
        return rune::prefs::Prefs::memory();
    }
    rune::prefs::Prefs::at(crate::host::t3_home().join("userdata").join("desktop-rune-ui.json"))
}

fn acknowledged(s: &App) -> &[String] {
    s.ui.shell.chatgpt_welcome.acknowledged.get_or_init(|| prefs().get(ACKNOWLEDGED_KEY).unwrap_or_default())
}

/// A connected environment's provider on a shared plan: its key, provider name and environment label.
fn profiles(s: &App) -> Vec<(String, String, String)> {
    let mut found = Vec::new();
    for key in env::keys(s) {
        let Some(src) = env::source(s, &key) else { continue };
        let state = src.state();
        let Some(config) = state.config.as_ref().filter(|_| matches!(state.conn, s5_net::Conn::Open)) else { continue };
        let environment = config["environment"]["environmentId"].as_str().unwrap_or(&key).to_owned();
        for provider in config["providers"].as_array().into_iter().flatten().filter(|p| uses_chatgpt_sharing(p)) {
            let auth = &provider["auth"];
            let profile = auth["profileId"].as_str().or(auth["email"].as_str()).unwrap_or("default");
            let id = json!([environment, data::s(provider, "instanceId"), profile]).to_string();
            let name = provider["displayName"].as_str().unwrap_or("Codex").to_owned();
            found.push((id, name, env::label(s, &key)));
        }
    }
    found
}

/// usesChatGptSharing.
fn uses_chatgpt_sharing(provider: &Value) -> bool {
    data::s(&provider["auth"], "status") == "authenticated" && provider["auth"]["subscriptionSharing"] == true
}

fn next(s: &App) -> Option<(String, String, String)> {
    let seen = acknowledged(s).to_vec();
    profiles(s).into_iter().find(|(id, ..)| !seen.contains(id))
}

fn dismiss(s: &mut App) {
    let Some((id, ..)) = next(s) else { return };
    let ui = &mut s.ui.shell.chatgpt_welcome;
    let Some(list) = ui.acknowledged.get_mut() else { return };
    list.push(id);
    let _ = prefs().set(ACKNOWLEDGED_KEY, &*list);
}

/// Marks every current profile welcomed, for scenes that show the plan elsewhere.
pub(crate) fn acknowledge_all(s: &mut App) {
    while next(s).is_some() {
        dismiss(s);
    }
}

pub fn view(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some((_, provider, environment)) = next(s) else { return };
    let muted = t.c.muted_foreground;
    let manage = kit::Button::new("Manage usage")
        .variant("ghost-muted")
        .size("sm")
        .build(t)
        .child(icon(&crate::icons::EXTERNAL_LINK, 14.0).color(muted))
        .on_press_tx(|_: &mut App, tx: &mut Tx| {
            let _ = tx.open_url(CHATGPT_USAGE_URL);
        });
    kit::Dialog::new("chatgpt-welcome", "Your ChatGPT plan is connected")
        .open(true)
        .title_icon(icon(&crate::chat::composer::provider_icons::OPENAI, 32.0).color(t.c.foreground))
        .description("Eligible usage in T3 Code uses your ChatGPT plan. Manage your shared usage and any credit settings in ChatGPT.")
        .on_close(dismiss)
        .child(text(format!("{provider} on {environment}")).font(theme::TEXT_XS).color(muted))
        .child(row().w_full().gap(8.0).justify_end().child(manage).child(kit::Button::new("Continue").on_press(dismiss).build(t)))
        .build(t, layers);
}

pub fn scene(s: &mut App) {
    if let data::Source::Fixture(state) = &mut s.src
        && let Some(config) = state.config.as_mut()
    {
        for p in config["providers"].as_array_mut().into_iter().flatten().filter(|p| p["instanceId"] == "codex") {
            p["auth"] = json!({"status": "authenticated", "type": "chatgpt", "subscriptionSharing": true, "email": "dev@example.com"});
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn each_shared_plan_profile_is_welcomed_once() {
        let mut s = App::new();
        assert!(next(&s).is_none(), "FAILED: no shared plan, no dialog");
        scene(&mut s);
        let (id, provider, _) = next(&s).expect("FAILED: a shared plan is welcomed");
        assert_eq!(provider, "Codex");
        assert!(id.ends_with(r#""codex","dev@example.com"]"#), "{id}");
        dismiss(&mut s);
        assert!(next(&s).is_none(), "FAILED: Continue or closing acknowledges it");
        if let data::Source::Fixture(state) = &mut s.src
            && let Some(config) = state.config.as_mut()
        {
            for p in config["providers"].as_array_mut().into_iter().flatten().filter(|p| p["instanceId"] == "codex") {
                p["auth"]["email"] = json!("other@example.com");
            }
        }
        assert!(next(&s).is_some(), "FAILED: another profile is welcomed in turn");
        if let data::Source::Fixture(state) = &mut s.src {
            state.conn = s5_net::Conn::Connecting;
        }
        assert!(next(&s).is_none(), "FAILED: only connected environments count");
    }
}
