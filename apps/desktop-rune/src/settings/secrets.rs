//! Web `SecretsSettingsPanel` and `SecretProviderDialog`: the environment's secret providers
//! (1Password accounts) that provider variables read from, behind `T3CODE_FEATURE_SECRETS`.
use std::collections::HashSet;
use std::sync::OnceLock;

use super::controls as c;
use crate::kit::{Button, Dialog, Input};
use crate::theme::{self, AlphaMul, T};
use crate::App;
use rune::{El, Icon, TextEdit, col, row, text};
use serde_json::{Value, json};

static ONE_PASSWORD: Icon = Icon {
    name: "one-password",
    view: 24.0,
    body: r#"<path d="M12 0c6.627 0 12 5.373 12 12 0 6.628-5.373 12-12 12S0 18.628 0 12C0 5.373 5.373 0 12 0m-.893 4.86c-.485 0-.727.001-.913.095a.87.87 0 0 0-.378.379c-.094.185-.095.428-.095.912v2.747c0 .12 0 .182.016.238q.02.075.065.138a1 1 0 0 0 .175.162l.695.564c.113.092.17.139.19.194a.22.22 0 0 1 0 .15c-.02.056-.077.102-.19.194l-.695.564a1 1 0 0 0-.175.162.4.4 0 0 0-.065.138 1 1 0 0 0-.016.238v6.019c0 .485 0 .728.095.913a.87.87 0 0 0 .378.378c.186.094.428.094.913.094h1.786c.485 0 .727 0 .913-.094a.87.87 0 0 0 .378-.378c.095-.185.095-.428.095-.913v-2.747c0-.12 0-.182-.016-.238a.4.4 0 0 0-.065-.138 1 1 0 0 0-.175-.162l-.695-.564c-.113-.092-.17-.138-.191-.193a.22.22 0 0 1 0-.152c.02-.055.078-.1.19-.193l.696-.564a1 1 0 0 0 .175-.162.4.4 0 0 0 .065-.138 1 1 0 0 0 .016-.238V6.246c0-.484 0-.727-.095-.912a.87.87 0 0 0-.378-.379c-.186-.094-.428-.094-.913-.094Z" fill="currentColor"/>"#,
};

#[derive(Default)]
pub struct Ui {
    /// The open dialog: `None` adds a provider, `Some(id)` edits that one.
    open: Option<Option<String>>,
    name: TextEdit,
    account: TextEdit,
    submitted: bool,
    /// The provider the remove confirmation is for: id and label.
    remove: Option<(String, String)>,
    revealed: HashSet<String>,
}

/// Web `isSecretsFeatureEnabled`: the page and its nav entry exist only with the flag on.
pub fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        std::env::var("T3CODE_FEATURE_SECRETS").is_ok_and(|v| matches!(v.trim().to_lowercase().as_str(), "1" | "true"))
    })
}

/// `classifySecretProvider`: a 1Password config whose account still decodes, else unsupported.
fn account(provider: &Value) -> Option<String> {
    if provider["kind"] != "one_password" {
        return None;
    }
    let account = provider["config"]["account"].as_str()?;
    account_issue(account).is_none().then(|| account.to_owned())
}

fn account_issue(account: &str) -> Option<&'static str> {
    let mut chars = account.chars();
    let shaped = chars.next().is_some_and(|c| c.is_ascii_alphanumeric())
        && chars.all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'));
    (!shaped || account.len() > 253).then_some("1Password account must be a shorthand, sign-in address, or ID with no spaces.")
}

/// `secretProviderIdFromName`: stable and readable in settings.json, so re-adding a provider under
/// the same name restores its variables.
fn id_from_name(name: &str) -> Option<String> {
    let mut slug = String::new();
    for c in name.to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('-') {
            slug.push('-');
        }
    }
    let slug: String = slug.trim_matches('-').chars().take(64).collect();
    let slug = slug.trim_end_matches('-');
    (!slug.is_empty()).then(|| slug.to_owned())
}

#[derive(Debug, Default, PartialEq)]
struct Issues {
    name: Option<String>,
    account: Option<String>,
}

/// `secretProviderFromDraft`: the entry a draft adds or replaces, or every field that blocks it.
fn from_draft(name: &str, account: &str, existing: &[String], editing: Option<&str>) -> Result<(String, Value), Issues> {
    let (label, account) = (name.trim(), account.trim());
    let mut issues = Issues::default();
    let mut id = None;
    if label.is_empty() {
        issues.name = Some("Enter a name.".into());
    } else if label.chars().count() > 64 {
        issues.name = Some("Expected a value with a length of at most 64".into());
    } else {
        id = editing.map(str::to_owned).or_else(|| id_from_name(label));
        match &id {
            None => issues.name = Some("The name needs at least one letter or digit.".into()),
            Some(id) if editing.is_none() && existing.contains(id) => {
                issues.name = Some("A secret provider with this name exists.".into())
            }
            _ => {}
        }
    }
    if account.is_empty() {
        issues.account = Some("Enter the 1Password account.".into());
    } else if let Some(issue) = account_issue(account) {
        issues.account = Some(issue.into());
    }
    match id {
        Some(id) if issues == Issues::default() => {
            Ok((id, json!({"kind": "one_password", "label": label, "config": {"account": account}})))
        }
        _ => Err(issues),
    }
}

fn providers(s: &App) -> Vec<(String, Value)> {
    super::server_settings(s)["secretProviders"]
        .as_object()
        .into_iter()
        .flatten()
        .map(|(id, provider)| (id.clone(), provider.clone()))
        .collect()
}

fn environment_label(s: &App) -> String {
    s.state().config.as_ref().and_then(|c| c["environment"]["label"].as_str()).unwrap_or("Environment").to_owned()
}

fn connected(s: &App) -> bool {
    matches!(s.src, crate::data::Source::Fixture(_)) || matches!(s.state().conn, s5_net::Conn::Open)
}

/// Web's provider access: connected, the server config loaded and `providers:manage` granted;
/// project scope reads only.
fn editable(s: &App) -> bool {
    connected(s)
        && s.state().config.is_some()
        && super::permissions::can(s, "providers:manage")
        && s.ui.settings.project.is_none()
}

fn note(message: String, t: &T) -> El {
    c::card(
        vec![row().w_full().px(16.0).py(12.0).child(text(message).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap())],
        t,
    )
}

fn settings_row(t: &T, title: El, description: El, control: Option<El>) -> El {
    let copy = col().grow(1.0).min_w(0.0).gap(4.0).child(title).child(description);
    let mut out = row().w_full().px(16.0).py(12.0).gap(32.0).center_y().child(copy);
    if let Some(control) = control {
        out = out.child(control);
    }
    out
}

fn open_dialog(s: &mut App, editing: Option<String>) {
    let provider = editing.as_ref().and_then(|id| providers(s).into_iter().find(|(p, _)| p == id)).map(|(_, p)| p);
    let ui = &mut s.ui.settings.secrets;
    ui.name = TextEdit::new(provider.as_ref().and_then(|p| p["label"].as_str()).unwrap_or(""));
    ui.name.focused = true;
    ui.account = TextEdit::new(provider.as_ref().and_then(account).unwrap_or_default());
    ui.submitted = false;
    ui.open = Some(editing);
}

pub fn view(s: &App, t: &T) -> El {
    let config = s.state().config.as_ref();
    if !connected(s) {
        return note("Connect an environment to manage the secret providers its provider variables read from.".into(), t);
    }
    if config.is_some_and(|c| c["secretProviderSettings"] != true) {
        return note(format!("Update T3 Code on {} to manage its secret providers.", environment_label(s)), t);
    }
    let editable = editable(s);
    let mut rows = vec![];
    if s.ui.settings.project.is_some() {
        rows.push(
            row().w_full().px(16.0).pt(12.0).child(
                text("Environment-wide setting. Select an environment to change it.").font(theme::TEXT_XS).color(t.c.muted_foreground),
            ),
        );
    }
    let entries = providers(s);
    if entries.is_empty() {
        rows.push(settings_row(
            t,
            text("No secret providers configured.").font(theme::SM_MEDIUM).color(t.c.foreground),
            text("Add a 1Password account, then pick it as the source of a provider variable.")
                .font(theme::TEXT_XS.lh(18.0))
                .color(t.c.muted_foreground)
                .wrap(),
            None,
        ));
    }
    for (id, provider) in entries {
        let label = provider["label"].as_str().unwrap_or(&id).to_owned();
        let account = account(&provider);
        let mut title = row().min_w(0.0).gap(8.0).center_y();
        if account.is_some() {
            title = title.child(rune::icon(&ONE_PASSWORD, 18.0).color(t.c.foreground.alpha_mul(0.8)));
        }
        title = title.child(text(label.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).wrap());
        let description = match &account {
            Some(account) => {
                let revealed = s.ui.settings.secrets.revealed.contains(&id);
                let shown = if revealed { account.clone() } else { crate::pages::usage_limits::redacted(account) };
                let toggle = id.clone();
                row().min_w(0.0).gap(4.0).center_y().child(text("1Password ·").font(theme::TEXT_XS).color(t.c.muted_foreground)).child(
                    text(shown)
                        .font(theme::mono(theme::TEXT_XS))
                        .color(t.c.muted_foreground.alpha_mul(if revealed { 1.0 } else { 0.6 }))
                        .truncate()
                        .key(format!("secret-account-{id}"))
                        .focusable(true)
                        .role("button")
                        .label("Toggle account visibility")
                        .on_press(move |s: &mut App| {
                            let revealed = &mut s.ui.settings.secrets.revealed;
                            if !revealed.remove(&toggle) {
                                revealed.insert(toggle.clone());
                            }
                        }),
                )
            }
            None => row().child(
                text(format!("Unsupported provider ({})", provider["kind"].as_str().unwrap_or("")))
                    .font(theme::TEXT_XS)
                    .color(t.c.muted_foreground),
            ),
        };
        let control = editable.then(|| {
            let mut actions = row().gap(4.0).center_y();
            if account.is_some() {
                let edit = id.clone();
                actions = actions.child(
                    Button::new("Edit").variant("ghost").size("xs").build(t).on_press(move |s: &mut App| open_dialog(s, Some(edit.clone()))),
                );
            }
            let (remove, name) = (id.clone(), label.clone());
            actions.child(
                Button::new("Remove")
                    .variant("ghost")
                    .size("xs")
                    .build(t)
                    .on_press(move |s: &mut App| s.ui.settings.secrets.remove = Some((remove.clone(), name.clone()))),
            )
        });
        rows.push(settings_row(t, title, description, control));
    }
    if editable {
        let add = Button::new("Add provider")
            .variant("outline")
            .size("xs")
            .icon(&crate::icons::PLUS)
            .build(t)
            .on_press(|s: &mut App| open_dialog(s, None));
        c::section_with_actions("", vec![add], rows, t)
    } else {
        c::card(rows, t)
    }
}

fn save(s: &mut App) {
    let existing: Vec<String> = providers(s).into_iter().map(|(id, _)| id).collect();
    let ui = &mut s.ui.settings.secrets;
    ui.submitted = true;
    let Some(editing) = ui.open.clone() else { return };
    let Ok((id, config)) = from_draft(&ui.name.text, &ui.account.text, &existing, editing.as_deref()) else { return };
    ui.open = None;
    super::request(s, "server.updateSettings", json!({"patch": {"secretProviders": {id: config}}}));
}

fn field(label: &str, input: El, error: Option<String>, help: Option<&str>, t: &T) -> El {
    let mut out = col().w_full().gap(6.0).child(text(label.to_owned()).font(theme::SM_MEDIUM).color(t.c.contrast_foreground)).child(input);
    if let Some(error) = error {
        out = out.child(text(error).font(theme::TEXT_XS).color(t.c.destructive).role("alert").wrap());
    }
    if let Some(help) = help {
        out = out.child(text(help.to_owned()).font(theme::TEXT_XS).color(t.c.contrast_muted_foreground).wrap());
    }
    out
}

pub fn overlays(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.settings.secrets;
    if let Some((id, label)) = &ui.remove {
        let id = id.clone();
        Dialog::new("remove-secret-provider", format!("Remove {label}?"))
            .open(true)
            .alert()
            .description(format!(
                "Provider variables that read from {label} stop resolving until a provider with the same name is added again. The secrets themselves are untouched."
            ))
            .on_close(|s: &mut App| s.ui.settings.secrets.remove = None)
            .child(
                row()
                    .gap(8.0)
                    .child(Button::new("Cancel").variant("outline").on_press(|s: &mut App| s.ui.settings.secrets.remove = None).build(t))
                    .child(
                        Button::new("Remove provider")
                            .variant("destructive")
                            .on_press(move |s: &mut App| {
                                s.ui.settings.secrets.remove = None;
                                super::request(s, "server.updateSettings", json!({"patch": {"secretProviders": {id.clone(): null}}}));
                            })
                            .build(t),
                    ),
            )
            .build(t, layers);
    }
    let Some(editing) = &ui.open else { return };
    let existing: Vec<String> = providers(s).into_iter().map(|(id, _)| id).collect();
    let issues = from_draft(&ui.name.text, &ui.account.text, &existing, editing.as_deref()).err();
    let shown = |pick: fn(&Issues) -> &Option<String>| issues.as_ref().filter(|_| ui.submitted).and_then(|i| pick(i).clone());
    let (name_error, account_error) = (shown(|i| &i.name), shown(|i| &i.account));
    let title = match editing.as_ref().and_then(|id| providers(s).into_iter().find(|(p, _)| p == id)) {
        Some((_, provider)) => format!("Edit {}", provider["label"].as_str().unwrap_or("")),
        None => "Add a 1Password provider".to_owned(),
    };
    let name = Input::new(&ui.name)
        .placeholder("Personal")
        .invalid(name_error.is_some())
        .on_edit(|s: &mut App| &mut s.ui.settings.secrets.name)
        .on_submit(|s: &mut App, _| save(s))
        .build(t)
        .key("secret-provider-name")
        .single_line();
    let account = Input::new(&ui.account)
        .placeholder("my.1password.com")
        .invalid(account_error.is_some())
        .on_edit(|s: &mut App| &mut s.ui.settings.secrets.account)
        .on_submit(|s: &mut App, _| save(s))
        .build(t)
        .key("secret-provider-account")
        .single_line();
    let blocked = ui.submitted && issues.is_some();
    Dialog::new("secret-provider", title)
        .open(true)
        .width(448.0)
        .description(format!(
            "Provider variables on {} can read values from it by name. Values are read with the op CLI on that server when a provider starts.",
            environment_label(s)
        ))
        .on_close(|s: &mut App| s.ui.settings.secrets.open = None)
        .child(
            col()
                .w_full()
                .gap(16.0)
                .child(field("Name", name, name_error, None, t))
                .child(field(
                    "Account",
                    account,
                    account_error,
                    Some("The account shorthand, sign-in address without https://, or ID that op account list shows."),
                    t,
                )),
        )
        .child(
            row()
                .w_full()
                .gap(8.0)
                .justify_end()
                .child(Button::new("Cancel").variant("outline").on_press(|s: &mut App| s.ui.settings.secrets.open = None).build(t))
                .child(
                    Button::new(if editing.is_some() { "Save changes" } else { "Add provider" })
                        .disabled(blocked)
                        .on_press(|s: &mut App| save(s))
                        .build(t),
                ),
        )
        .build(t, layers);
}

/// Escape closes the topmost secrets dialog first.
pub fn escape(s: &mut App) -> bool {
    let ui = &mut s.ui.settings.secrets;
    ui.remove.take().is_some() || ui.open.take().is_some()
}

/// Headless scenes: one 1Password account and one provider kind this client doesn't know.
pub fn scene(s: &mut App, dialog: bool) {
    let mut settings = super::server_settings(s).clone();
    if !settings.is_object() {
        settings = json!({});
    }
    settings["secretProviders"] = json!({
        "personal": {"kind": "one_password", "label": "Personal", "config": {"account": "my.1password.com"}},
        "vault": {"kind": "hashicorp_vault", "label": "Team vault"}
    });
    s.ui.settings.optimistic_settings = Some(settings);
    if let crate::data::Source::Fixture(state) = &mut s.src {
        state.config.get_or_insert_with(|| json!({}))["secretProviderSettings"] = json!(true);
    }
    if dialog {
        open_dialog(s, None);
        s.ui.settings.secrets.name = TextEdit::new("Work");
        s.ui.settings.secrets.account = TextEdit::new("bad account");
        s.ui.settings.secrets.submitted = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_draft_becomes_a_slugged_one_password_entry_like_web() {
        let (id, config) = from_draft("  My Team's Vault! ", " my.1password.com ", &[], None).unwrap();
        assert_eq!(id, "my-team-s-vault");
        assert_eq!(config, json!({"kind": "one_password", "label": "My Team's Vault!", "config": {"account": "my.1password.com"}}));
        assert_eq!(from_draft("1Password", "abc", &[], None).unwrap().0, "1password");
        // Editing keeps the id even when the name changes.
        assert_eq!(from_draft("Renamed", "abc", &["personal".into()], Some("personal")).unwrap().0, "personal");
    }

    #[test]
    fn every_blocking_field_reports_its_own_message() {
        assert_eq!(
            from_draft("", "", &[], None).unwrap_err(),
            Issues { name: Some("Enter a name.".into()), account: Some("Enter the 1Password account.".into()) }
        );
        assert_eq!(from_draft("!!!", "abc", &[], None).unwrap_err().name.as_deref(), Some("The name needs at least one letter or digit."));
        assert_eq!(
            from_draft("Personal", "abc", &["personal".into()], None).unwrap_err().name.as_deref(),
            Some("A secret provider with this name exists.")
        );
        for bad in ["-leading", "has space", "a/b"] {
            assert!(from_draft("Ok", bad, &[], None).unwrap_err().account.is_some(), "{bad}");
        }
        assert!(from_draft("Ok", "user@example.com", &[], None).is_err());
        assert!(from_draft("Ok", "ABCDEF123", &[], None).is_ok());
    }

    #[test]
    fn slugs_cap_at_64_without_a_trailing_dash() {
        let long = format!("{} tail", "a".repeat(63));
        assert_eq!(id_from_name(&long).unwrap(), "a".repeat(63));
        assert_eq!(id_from_name("--Hello  World--").unwrap(), "hello-world");
        assert_eq!(id_from_name("ëëë"), None);
    }

    #[test]
    fn only_decodable_one_password_configs_show_an_account() {
        assert_eq!(account(&json!({"kind": "one_password", "label": "P", "config": {"account": "my.1password.com"}})).as_deref(), Some("my.1password.com"));
        assert_eq!(account(&json!({"kind": "one_password", "label": "P", "config": {"account": "bad account"}})), None);
        assert_eq!(account(&json!({"kind": "one_password", "label": "P"})), None);
        assert_eq!(account(&json!({"kind": "vault", "label": "P", "config": {"account": "abc"}})), None);
    }
}
