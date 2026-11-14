//! The Source control page's server scan (web `SourceControlSettingsPanel`): version control
//! tools and hosting integrations found on the server. The Repositories and Writing & review
//! sections around it are ordinary `pages.json` rows.

use super::{controls as c, request, server_settings};
use crate::{
    App, icons,
    theme::{self, AlphaMul, T},
};
use rune::{El, TextEdit, col, el, row, text};
use serde_json::{Value, json};
use std::collections::HashSet;

#[derive(Default)]
pub struct Ui {
    open: HashSet<String>,
    /// The GitHub token form's host; `None` until edited, showing the public host.
    host: Option<TextEdit>,
    token: TextEdit,
    revealed: bool,
    /// A host settings write is in flight; its reply rescans, like web's `onSaved`.
    pub rescan: bool,
    app: AppSignIn,
    app_revealed: bool,
}

/// Web `SourceControlAppSignInSettings`'s state: GitHub's device flow, run by the server.
#[derive(Default, Debug, PartialEq)]
enum AppSignIn {
    #[default]
    Idle,
    Starting(u64),
    Waiting { code: String, uri: String, expires_at: i64, wait: u64 },
    Failed(String),
}

const APP_NAME: &str = "T3 Code for GitHub";
const APP_HOST: &str = "github.com";
const APP_INSTALL: &str = "https://github.com/apps/t3-code-for-github/installations/new";
const APP_REVOKE: &str = "https://github.com/settings/apps/authorizations";

fn app_input() -> Value {
    json!({"provider": "github", "host": APP_HOST})
}

/// The next sign-in state for a reply; a reply for an attempt that's no longer current is dropped.
fn app_reply(state: &AppSignIn, id: u64, reply: Result<Value, Value>) -> Option<AppSignIn> {
    let failed = |e: &Value| AppSignIn::Failed(super::failure_detail(e).unwrap_or_else(|| "GitHub sign-in failed. Try again.".into()));
    match state {
        AppSignIn::Starting(start) if *start == id => Some(match reply {
            Ok(prompt) => AppSignIn::Waiting {
                code: prompt["userCode"].as_str().unwrap_or("").to_owned(),
                uri: prompt["verificationUri"].as_str().unwrap_or("https://github.com/login/device").to_owned(),
                expires_at: prompt["expiresAt"].as_i64().unwrap_or(0),
                wait: 0,
            },
            Err(e) => failed(&e),
        }),
        AppSignIn::Waiting { wait, .. } if *wait == id => Some(match reply {
            Ok(_) => AppSignIn::Idle,
            Err(e) => failed(&e),
        }),
        _ => None,
    }
}

/// Settles the device flow's start and wait replies; a finished sign-in rescans.
pub fn poll(s: &mut App) -> bool {
    let pending = match &s.ui.settings.source_control.app {
        AppSignIn::Starting(id) => *id,
        AppSignIn::Waiting { wait, .. } if *wait != 0 => *wait,
        _ => return false,
    };
    let Some(reply) = s.src.net().and_then(|n| n.take_reply(pending)) else { return false };
    let succeeded = reply.is_ok();
    let Some(mut next) = app_reply(&s.ui.settings.source_control.app, pending, reply) else { return false };
    if let AppSignIn::Waiting { wait, .. } = &mut next
        && let Some(net) = s.src.net()
    {
        *wait = net.request("sourceControl.signInWait", app_input());
    }
    let done = succeeded && next == AppSignIn::Idle;
    s.ui.settings.source_control.app = next;
    if done {
        scan(s);
    }
    true
}

/// Web's `toLocaleTimeString(undefined, {hour: "numeric", minute: "2-digit"})` in en-US.
fn clock(at: i64) -> String {
    let minutes = crate::data::Zone::Local.wall(at).div_euclid(60_000).rem_euclid(24 * 60);
    let (hour, minute) = (minutes / 60, minutes % 60);
    format!("{}:{minute:02} {}", (hour + 11) % 12 + 1, if hour < 12 { "AM" } else { "PM" })
}

fn link(label: &'static str, url: &'static str, t: &T) -> El {
    crate::kit::Button::new(label).variant("link").size("xs").icon(&icons::EXTERNAL_LINK).on_press(move |s: &mut App| super::auth::open_url(s, url)).build(t)
}

/// A band under the card's header, with its top rule.
fn band(rule: rune::Color, fill: rune::Color, py: f32, body: El) -> El {
    col().w_full().child(el().h(1.0).w_full().fill(rule)).child(col().w_full().px(12.0).py(py).fill(fill).child(body))
}

/// The GitHub App card, shown when the server can run its device flow (`appSignIn`).
fn app_sign_in(s: &App, t: &T, account: Option<&str>) -> El {
    let ui = &s.ui.settings.source_control;
    let hosts = saved(s, "github");
    let signed_in = hosts["signIns"][APP_HOST].as_str().is_some_and(|v| !v.is_empty());
    let token_overrides = has_secret(&hosts["tokens"][APP_HOST]);
    let failed = match &ui.app {
        AppSignIn::Failed(message) if !signed_in => Some(message.clone()),
        _ => None,
    };
    let waiting = match &ui.app {
        AppSignIn::Waiting { code, uri, expires_at, .. } if !signed_in => Some((code.clone(), uri.clone(), *expires_at)),
        _ => None,
    };
    let (subtitle, action): (El, El) = if signed_in {
        let subtitle = match account.filter(|_| !token_overrides) {
            Some(account) => text(format!("Signed in as {}", login(account, ui.app_revealed)))
                .role("button")
                .label(if ui.app_revealed { "Hide account" } else { "Show account" })
                .cursor(rune::Cursor::Pointer)
                .on_press(|s: &mut App| s.ui.settings.source_control.app_revealed ^= true),
            None if token_overrides => text("Signed in, but the token saved below is used while it can be read."),
            None => text("Signed in"),
        };
        let action = crate::kit::Button::new("Sign out")
            .variant("outline")
            .size("xs")
            .disabled(!super::permissions::allowed(s, "sourceControl.signInStart"))
            .on_press(|s: &mut App| {
                s.ui.settings.source_control.app = AppSignIn::Idle;
                let mut sign_ins = saved(s, "github")["signIns"].as_object().cloned().unwrap_or_default();
                sign_ins.insert(APP_HOST.into(), json!(""));
                s.ui.settings.source_control.rescan = true;
                request(s, "server.updateSettings", json!({"patch": {"sourceControlHosts": {"github": {"signIns": sign_ins}}}}));
            })
            .build(t);
        (subtitle, action)
    } else if waiting.is_some() {
        let action = crate::kit::Button::new("Cancel")
            .variant("ghost")
            .size("xs")
            .on_press(|s: &mut App| {
                s.ui.settings.source_control.app = AppSignIn::Idle;
                request(s, "sourceControl.signInCancel", app_input());
            })
            .build(t);
        (text("Approve the app on GitHub to finish signing in."), action)
    } else {
        let starting = matches!(ui.app, AppSignIn::Starting(_));
        let action = crate::kit::Button::new(if failed.is_some() { "Try again" } else { "Sign in with GitHub" })
            .size("xs")
            .disabled(starting || !super::permissions::allowed(s, "sourceControl.signInStart"))
            .on_press(|s: &mut App| {
                if let Some(net) = s.src.net() {
                    let id = net.request("sourceControl.signInStart", app_input());
                    s.ui.settings.source_control.app = AppSignIn::Starting(id);
                }
            })
            .build(t);
        (text("A far higher GitHub API rate limit than a personal token or gh login."), action)
    };
    let logo = super::host_logos::layers("github").into_iter().flatten().fold(el().size(20.0, 20.0), |logo, &(glyph, _)| logo.child(rune::icon(glyph, 20.0).color(t.c.foreground).absolute(0.0, 0.0)));
    let mut mark = el().size(40.0, 40.0).radius(10.0).fill(t.c.muted).center().child(logo);
    if signed_in {
        mark = mark.child(
            el().size(16.0, 16.0).radius(8.0).fill(t.c.success).border(2.0, t.c.card).center().absolute(28.0, 28.0).child(rune::icon(&icons::CHECK, 10.0).color(rune::Color::WHITE)),
        );
    }
    let mut card = col()
        .w_full()
        .radius(12.0)
        .border(1.0, if failed.is_some() { t.c.destructive.alpha_mul(0.4) } else { t.c.border })
        .fill(t.c.card)
        .clip_rounded(12.0)
        .child(
            row()
                .w_full()
                .padding(12.0)
                .gap(12.0)
                .center_y()
                .child(mark)
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(2.0)
                        .child(text(APP_NAME).font(theme::SM_MEDIUM).color(t.c.foreground))
                        .child(subtitle.font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()),
                )
                .child(action),
        );
    let muted = t.c.muted.alpha_mul(0.4);
    if let Some((code, uri, expires_at)) = waiting {
        let mut cells = row().gap(4.0).center_y().label(code.clone());
        for c in code.chars() {
            cells = cells.child(if c == '-' {
                text("\u{2013}").font(theme::MONO_SM).color(t.c.muted_foreground).px(2.0)
            } else {
                el().size(32.0, 40.0)
                    .radius(6.0)
                    .border(1.0, t.c.border)
                    .fill(t.c.background)
                    .center()
                    .child(text(c.to_string()).font(theme::weight(theme::mono(theme::TEXT_LG), theme::MEDIUM)).color(t.c.foreground))
            });
        }
        let (copy_code, open_code) = (code.clone(), code);
        cells = cells.child(
            crate::kit::Button::new("Copy code")
                .variant("ghost")
                .size("icon-xs")
                .icon(&icons::COPY)
                .build(t)
                .on_press_tx(move |_: &mut App, tx: &mut rune::Tx| tx.copy(copy_code.clone())),
        );
        let open = crate::kit::Button::new("Copy code & open GitHub").size("sm").icon(&icons::EXTERNAL_LINK).build(t).on_press_tx(move |s: &mut App, tx: &mut rune::Tx| {
            tx.copy(open_code.clone());
            super::auth::open_url(s, &uri);
        });
        let status = row()
            .gap(6.0)
            .center_y()
            .child(el().size(6.0, 6.0).radius(3.0).fill(t.c.warning))
            .child(small(format!("Waiting for approval · code expires at {}", clock(expires_at)), t));
        let body = row().w_full().gap(24.0).center_y().child(cells).child(el().grow(1.0)).child(col().gap(8.0).items_end().child(open).child(status));
        card = card.child(band(t.c.border, muted, 16.0, body));
    }
    if signed_in {
        let body = row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(small("Reaches repositories in accounts and organizations that installed the app.", t).grow(1.0).min_w(0.0))
            .child(link("Install on an organization", APP_INSTALL, t))
            .child(link("Manage on GitHub", APP_REVOKE, t));
        card = card.child(band(t.c.border, muted, 8.0, body));
    }
    if let Some(message) = failed {
        let body = text(message).font(theme::TEXT_XS).color(t.c.destructive).wrap().role("alert");
        card = card.child(band(t.c.destructive.alpha_mul(0.2), t.c.destructive.alpha_mul(0.05), 8.0, body));
    }
    card
}

pub const DISCOVERY: &str = "server.discoverSourceControl";

pub fn scan(s: &mut App) {
    request(s, DISCOVERY, json!({}));
}

/// `Schema.Option` fields arrive as `{_tag, value}` or as the bare value.
fn opt(v: &Value) -> Option<&str> {
    v["value"].as_str().or(v.as_str()).filter(|v| !v.is_empty())
}

fn is_provider(item: &Value) -> bool {
    item.get("auth").is_some()
}

fn not_ready(item: &Value) -> bool {
    !is_provider(item) && item["implemented"] == false
}

fn available(item: &Value) -> bool {
    item["status"] == "available"
}

fn enabled(item: &Value) -> bool {
    if is_provider(item) {
        available(item) && item["auth"]["status"] == "authenticated"
    } else {
        available(item) && item["implemented"] == true
    }
}

/// `itemSummary`: the line under the label.
pub fn summary(item: &Value) -> String {
    let label = item["label"].as_str().unwrap_or("");
    let hint = item["installHint"].as_str().unwrap_or("");
    if not_ready(item) {
        return format!("Support for {label} is coming soon.");
    }
    if !available(item) {
        return format!("Not available on this server: {hint}");
    }
    if !is_provider(item) {
        return "Available".into();
    }
    let auth = &item["auth"];
    let detail = opt(&auth["detail"]);
    match auth["status"].as_str() {
        Some("authenticated") => {
            let mut line = "Authenticated".to_owned();
            if let Some(account) = opt(&auth["account"]) {
                line.push_str(&format!(" as {account}"));
            }
            if let Some(detail) = detail {
                line.push_str(&format!(" · {detail}"));
            }
            line
        }
        Some("unauthenticated") if item["executable"].as_str().is_none() => format!("Available. {hint}"),
        Some("unauthenticated")
            if auth["accounts"].as_array().is_some_and(|a| a.iter().any(|e| e["authenticated"] == true)) =>
        {
            detail.map(str::to_owned).unwrap_or_else(|| format!("Every {label} host is turned off."))
        }
        Some("unauthenticated") => format!(
            "{label} is not authenticated on this server. Sign in or configure credentials using the {} tool on the server host to enable change request features.",
            item["executable"].as_str().unwrap_or("")
        ),
        _ => format!("Could not verify {label}. {}", detail.unwrap_or(hint)),
    }
}

fn dot(item: &Value, t: &T) -> rune::Color {
    if not_ready(item) || (is_provider(item) && !enabled(item)) {
        t.c.muted_foreground.alpha_mul(0.35)
    } else if !available(item) {
        t.c.warning
    } else {
        t.c.success
    }
}

fn item_row(s: &App, item: &Value, t: &T) -> El {
    let kind = item["kind"].as_str().unwrap_or("");
    let label = item["label"].as_str().unwrap_or("");
    let status = dot(item, t);
    let mark = super::host_logos::mark(kind, status, t).unwrap_or_else(|| el().size(8.0, 8.0).radius(4.0).fill(status));
    let mut heading = row()
        .gap(8.0)
        .center_y()
        .child(mark)
        .child(text(label.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground));
    if let Some(version) = opt(&item["version"]) {
        heading = heading.child(text(version.to_owned()).font(theme::MONO_XS).color(t.c.muted_foreground));
    }
    if not_ready(item) {
        heading = heading.child(crate::kit::badge("Coming Soon", "warning", "sm", t));
    }
    if is_provider(item) && item["auth"]["status"] == "unauthenticated" {
        heading = heading.child(crate::kit::badge("Not authenticated", "secondary", "sm", t));
    }
    let mut line = row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(12.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(heading)
                .child(text(summary(item)).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground.alpha_mul(0.8)).wrap()),
        );
    let panel = is_provider(item) && has_panel(kind);
    let expanded = panel && s.ui.settings.source_control.open.contains(kind);
    if panel {
        let owned = kind.to_owned();
        line = line.child(
            crate::kit::Button::new(format!("Toggle {label} details"))
                .variant("ghost-muted")
                .size("icon-xs")
                .icon(if expanded { &icons::CHEVRON_UP } else { &icons::CHEVRON_DOWN })
                .on_press(move |s: &mut App| {
                    let open = &mut s.ui.settings.source_control.open;
                    if !open.remove(&owned) {
                        open.insert(owned.clone());
                    }
                })
                .build(t)
                .expanded(expanded),
        );
    }
    if !not_ready(item) {
        line = line.child(
            crate::kit::Switch::new(enabled(item))
                .label(format!("{label} availability"))
                .hide_label()
                .disabled(true)
                .build(&crate::kit::theme(t)),
        );
    }
    let line = if not_ready(item) { line.opacity(0.8) } else { line };
    if !expanded {
        return line;
    }
    let body = match credentials(kind) {
        Some(credentials) => {
            let mut body = col().w_full().gap(24.0);
            if item["appSignIn"] == true {
                let divider = |t: &T| el().h(1.0).grow(1.0).fill(t.c.border);
                let or = text("Or use your own credentials").font(theme::TEXT_2XS).color(t.c.muted_foreground);
                body = body.child(app_sign_in(s, t, opt(&item["auth"]["account"]))).child(row().w_full().gap(12.0).center_y().child(divider(t)).child(or).child(divider(t)));
            }
            body = body.child(token_form(s, t, kind, label, &credentials));
            if available(item) {
                body = body.child(accounts(s, t, kind, label, &credentials, &item["auth"]));
            }
            body
        }
        None => host_fields(s, t, kind),
    };
    col().w_full().child(line).child(col().w_full().px(16.0).pt(4.0).pb(16.0).child(body))
}

/// Web `SourceControlHostCredentials` for the hosts that have them (GitHub).
struct Credentials {
    cli: &'static str,
    cli_label: &'static str,
    login: &'static str,
    env: &'static str,
    access: &'static str,
    public_host: &'static str,
}

fn credentials(kind: &str) -> Option<Credentials> {
    (kind == "github").then_some(Credentials {
        cli: "gh",
        cli_label: "GitHub CLI",
        login: "gh auth login",
        env: "GH_TOKEN",
        access: "read and write access to pull requests and contents",
        public_host: "github.com",
    })
}

/// A field of a host's settings schema (`ProviderSettingsForm` over `definition.settings`).
struct Field {
    key: &'static str,
    title: &'static str,
    description: &'static str,
    placeholder: &'static str,
    secret: bool,
}

fn fields(kind: &str) -> &'static [Field] {
    match kind {
        "bitbucket" => &[
            Field {
                key: "accessToken",
                title: "Access token",
                description: "Scoped to one repository, project, or workspace. Create it in that item's Bitbucket settings.",
                placeholder: "",
                secret: true,
            },
            Field {
                key: "email",
                title: "Atlassian account email",
                description: "With an API token instead of an access token.",
                placeholder: "you@example.com",
                secret: false,
            },
            Field {
                key: "apiToken",
                title: "API token",
                description: "Reaches every repository your Atlassian account can. Give it read and write access to repositories and pull requests, and read:user:bitbucket.",
                placeholder: "",
                secret: true,
            },
        ],
        "gitcafe" => &[Field {
            key: "token",
            title: "Token",
            description: "A git.cafe token, used before CAFE_TOKEN and the cafe login, so GitCafe works without the CLI signed in.",
            placeholder: "",
            secret: true,
        }],
        _ => &[],
    }
}

fn has_panel(kind: &str) -> bool {
    credentials(kind).is_some() || !fields(kind).is_empty()
}

fn saved<'a>(s: &'a App, kind: &str) -> &'a Value {
    &server_settings(s)["sourceControlHosts"][kind]
}

/// A saved secret arrives redacted (a non-empty string) or as a secret reference object.
fn has_secret(v: &Value) -> bool {
    v.as_str().is_some_and(|v| !v.is_empty()) || v.is_object()
}

fn save(s: &mut App, kind: &str, fields: Value) {
    s.ui.settings.source_control.rescan = true;
    request(s, "server.updateSettings", json!({"patch": {"sourceControlHosts": {kind: fields}}}));
}

/// `schost::<kind>::<field>` rows: what the field shows. Secrets never prefill.
pub fn value(s: &App, key: &str) -> Value {
    let Some((kind, field)) = key.split_once("::") else { return Value::Null };
    if let Some(host) = field.strip_prefix("account::") {
        return json!(saved(s, kind)["hosts"][host]["account"].as_str().unwrap_or(ACTIVE_ACCOUNT));
    }
    match fields(kind).iter().find(|f| f.key == field) {
        Some(f) if !f.secret => json!(saved(s, kind)[field].as_str().unwrap_or("")),
        _ => Value::Null,
    }
}

pub fn update(s: &mut App, key: &str, value: Value) {
    let Some((kind, field)) = key.split_once("::") else { return };
    if let Some(host) = field.strip_prefix("account::") {
        let account = value.as_str().filter(|a| *a != ACTIVE_ACCOUNT).map(str::to_owned);
        set_host(s, kind, host, None, Some(account));
        return;
    }
    // An empty commit keeps a stored secret; Remove is how one is cleared.
    if fields(kind).iter().any(|f| f.key == field && f.secret) && value.as_str().is_some_and(|v| v.trim().is_empty()) {
        return;
    }
    save(s, kind, json!({field: value}));
}

/// Web `SourceControlHostSettings`: the schema's fields as settings rows, and Remove once any
/// is saved.
fn host_fields(s: &App, t: &T, kind: &str) -> El {
    let mut body = col().w_full();
    for field in fields(kind) {
        let key = format!("schost::{kind}::{}", field.key);
        let control = if field.secret {
            secret_control(s, t, kind, field, &key)
        } else {
            c::input(s, t, &key, 224.0, field.placeholder, false)
        };
        body = body.child(
            row()
                .key(format!("row-{key}"))
                .w_full()
                .py(12.0)
                .gap(32.0)
                .center_y()
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .gap(4.0)
                        .child(text(field.title).font(theme::SM_MEDIUM).color(t.c.foreground))
                        .child(text(field.description).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap()),
                )
                .child(control),
        );
    }
    let stored = saved(s, kind);
    if fields(kind).iter().any(|f| has_secret(&stored[f.key])) {
        let owned = kind.to_owned();
        body = body.child(
            row().w_full().pt(8.0).child(el().grow(1.0)).child(
                crate::kit::Button::new("Remove")
                    .variant("outline")
                    .size("xs")
                    .on_press(move |s: &mut App| {
                        let cleared: serde_json::Map<String, Value> =
                            fields(&owned).iter().map(|f| (f.key.to_owned(), json!(""))).collect();
                        save(s, &owned, Value::Object(cleared));
                    })
                    .build(t),
            ),
        );
    }
    body
}

fn secret_control(s: &App, t: &T, kind: &str, field: &Field, key: &str) -> El {
    let configured = has_secret(&saved(s, kind)[field.key]);
    let mut control = row().w(224.0).gap(8.0).center_y().child(
        super::providers::secret_input(s, t, key, if configured { "Stored secret" } else { "Not set" }, false)
            .grow(1.0)
            .min_w(0.0)
            .label(field.title),
    );
    if configured {
        let (kind, name) = (kind.to_owned(), field.key);
        control = control.child(
            crate::kit::Button::new(format!("Clear {}", field.title))
                .variant("ghost-destructive")
                .size("icon-sm")
                .icon(&icons::X)
                .on_press(move |s: &mut App| save(s, &kind, json!({name: ""})))
                .build(t),
        );
    }
    control
}

fn small(copy: impl Into<String>, t: &T) -> El {
    text(copy.into()).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap()
}

fn caption(copy: &str, t: &T) -> El {
    text(copy.to_owned()).font(theme::XS_MEDIUM).color(t.c.foreground)
}

/// Web `SourceControlTokenSettings`: a token per hostname, used before the CLI's env var and login.
fn token_form(s: &App, t: &T, kind: &str, label: &str, cr: &Credentials) -> El {
    let ui = &s.ui.settings.source_control;
    let host_edit = ui.host.clone().unwrap_or_else(|| TextEdit::new(cr.public_host));
    let host = host_edit.text.trim().to_lowercase();
    let tokens = &saved(s, kind)["tokens"];
    let is_saved = has_secret(&tokens[&host]);
    let saved_hosts: Vec<&str> =
        tokens.as_object().into_iter().flatten().filter(|(_, v)| has_secret(v)).map(|(h, _)| h.as_str()).collect();
    let token = ui.token.text.trim().to_owned();
    let url_host = if host.is_empty() { cr.public_host.to_owned() } else { host.clone() };
    let url = format!("https://{url_host}/settings/personal-access-tokens/new");
    // Web's `max-w-2xl` paragraph; the link follows it instead of flowing inline.
    let intro = col()
        .w_full()
        .gap(2.0)
        .child(
            small(
                format!(
                    "A token saved here is used before {} and the {} login, so {label} works without the {}. Give it {}.",
                    cr.env, cr.cli, cr.cli_label, cr.access
                ),
                t,
            )
            .w_full()
            .max_w(672.0),
        )
        .child(
            crate::kit::Button::new("Create a token")
                .variant("link")
                .size("xs")
                .icon(&icons::EXTERNAL_LINK)
                .on_press(move |s: &mut App| super::auth::open_url(s, &url))
                .build(t)
                .self_start(),
        );
    let host_input = crate::kit::Input::new(&host_edit)
        .size("sm")
        .placeholder(cr.public_host)
        .on_edit(|s: &mut App| {
            let ui = &mut s.ui.settings.source_control;
            ui.host.get_or_insert_with(|| TextEdit::new("github.com"))
        })
        .build(t)
        .label("Host")
        .single_line();
    let owned = kind.to_owned();
    let submit = move |s: &mut App, _: &mut rune::Tx| submit_token(s, &owned);
    let token_input = crate::kit::Input::new(&ui.token)
        .size("sm")
        .placeholder(if is_saved { "Stored secret" } else { "Paste a token" })
        .on_edit(|s: &mut App| &mut s.ui.settings.source_control.token)
        .on_submit(submit)
        .build(t)
        .label(format!("{label} token"))
        .single_line()
        .secure();
    let fields = row()
        .w_full()
        .gap(12.0)
        .child(col().w(192.0).gap(6.0).child(caption("Host", t)).child(host_input))
        .child(col().grow(1.0).min_w(0.0).gap(6.0).child(caption("Token", t)).child(token_input));
    let status = if saved_hosts.is_empty() {
        format!("No token saved; the server uses {} or the {} login.", cr.env, cr.cli)
    } else {
        format!("Saved for {}.", saved_hosts.join(", "))
    };
    let mut buttons = row().gap(8.0);
    if is_saved {
        let (kind, host) = (kind.to_owned(), host.clone());
        buttons = buttons.child(
            crate::kit::Button::new("Remove")
                .variant("outline")
                .size("xs")
                .on_press(move |s: &mut App| save_token(s, &kind, &host, ""))
                .build(t),
        );
    }
    let owned = kind.to_owned();
    buttons = buttons.child(
        crate::kit::Button::new("Save")
            .size("xs")
            .disabled(host.is_empty() || token.is_empty())
            .on_press(move |s: &mut App| submit_token(s, &owned))
            .build(t),
    );
    col()
        .w_full()
        .gap(12.0)
        .child(intro)
        .child(fields)
        .child(row().w_full().gap(12.0).center_y().child(small(status, t).grow(1.0).min_w(0.0)).child(buttons))
}

fn submit_token(s: &mut App, kind: &str) {
    let ui = &s.ui.settings.source_control;
    let public = credentials(kind).map_or("", |c| c.public_host);
    let host = ui.host.as_ref().map_or(public.to_owned(), |e| e.text.trim().to_lowercase());
    let token = ui.token.text.trim().to_owned();
    if !host.is_empty() && !token.is_empty() {
        save_token(s, kind, &host, &token);
    }
}

fn save_token(s: &mut App, kind: &str, host: &str, token: &str) {
    s.ui.settings.source_control.token = TextEdit::new(String::new()).secure();
    save(s, kind, json!({"tokens": {host: token}}));
}

const ACTIVE_ACCOUNT: &str = "active cli account";

/// Web `groupHostAccounts`: per host, the signed-in logins one can pin, the broken ones, and an
/// environment variable that overrides them.
#[derive(Debug, PartialEq)]
struct Group {
    host: String,
    active: Option<String>,
    selectable: Vec<String>,
    broken: Vec<(String, Option<String>)>,
    env: Option<String>,
}

fn group_accounts(accounts: &[Value]) -> Vec<Group> {
    let mut hosts: Vec<&str> = Vec::new();
    for host in accounts.iter().filter_map(|a| a["host"].as_str()) {
        if !hosts.contains(&host) {
            hosts.push(host);
        }
    }
    hosts
        .into_iter()
        .map(|host| {
            let entries: Vec<&Value> = accounts.iter().filter(|a| a["host"] == host).collect();
            let stored: Vec<&Value> = entries.iter().copied().filter(|a| a["environmentVariable"].as_str().is_none()).collect();
            let selectable: Vec<&Value> = stored.iter().copied().filter(|a| a["authenticated"] == true).collect();
            let account = |a: &Value| a["account"].as_str().unwrap_or("").to_owned();
            Group {
                host: host.to_owned(),
                active: selectable.iter().find(|a| a["active"] == true).or(selectable.first()).map(|&a| account(a)),
                selectable: selectable.iter().map(|&a| account(a)).collect(),
                broken: stored
                    .iter()
                    .filter(|a| a["authenticated"] != true)
                    .map(|&a| (account(a), a["error"].as_str().map(str::to_owned)))
                    .collect(),
                env: entries.iter().find_map(|a| a["environmentVariable"].as_str().filter(|v| !v.is_empty()).map(str::to_owned)),
            }
        })
        .collect()
}

/// Web `nextHostAccountChoices`: a host that is on and unpinned is dropped from the map.
fn next_hosts(current: &Value, host: &str, enabled: Option<bool>, account: Option<Option<String>>) -> Value {
    let previous = &current[host];
    let enabled = enabled.or(previous["enabled"].as_bool()).unwrap_or(true);
    let account = match account {
        Some(account) => account,
        None => previous["account"].as_str().map(str::to_owned),
    };
    let mut next = current.as_object().cloned().unwrap_or_default();
    next.remove(host);
    if !enabled || account.is_some() {
        let mut choice = json!({"enabled": enabled});
        if let Some(account) = account {
            choice["account"] = json!(account);
        }
        next.insert(host.to_owned(), choice);
    }
    Value::Object(next)
}

fn set_host(s: &mut App, kind: &str, host: &str, enabled: Option<bool>, account: Option<Option<String>>) {
    let hosts = next_hosts(&saved(s, kind)["hosts"], host, enabled, account);
    save(s, kind, json!({"hosts": hosts}));
}

/// A login, masked until the eye reveals it (web blurs a placeholder of the same length).
fn login(account: &str, revealed: bool) -> String {
    if revealed { account.to_owned() } else { "\u{2022}".repeat(account.chars().count().clamp(4, 16)) }
}

/// Web `SourceControlAccountSettings`: which CLI login each host uses, or turning a host off.
fn accounts(s: &App, t: &T, kind: &str, label: &str, cr: &Credentials, auth: &Value) -> El {
    let groups = group_accounts(auth["accounts"].as_array().map(Vec::as_slice).unwrap_or(&[]));
    if groups.is_empty() {
        return small(format!("Sign in with {} on the server host, then rescan to choose accounts here.", cr.login), t);
    }
    let revealed = s.ui.settings.source_control.revealed;
    let hosts = &saved(s, kind)["hosts"];
    let mut body = col().w_full().gap(16.0).child(
        row()
            .w_full()
            .gap(12.0)
            .center_y()
            .child(small(format!("Choose which {} login each {label} host uses, or turn a host off.", cr.cli), t).grow(1.0).min_w(0.0))
            .child(
                crate::kit::Button::new(format!("{} {label} accounts", if revealed { "Hide" } else { "Reveal" }))
                    .variant("ghost-muted")
                    .size("icon-xs")
                    .icon(if revealed { &icons::EYE_OFF } else { &icons::EYE })
                    .on_press(|s: &mut App| s.ui.settings.source_control.revealed ^= true)
                    .build(t)
                    .checked(revealed),
            ),
    );
    for group in groups {
        let choice = &hosts[&group.host];
        let enabled = choice["enabled"].as_bool().unwrap_or(true);
        let pinned = choice["account"].as_str();
        let stale = pinned.is_some_and(|a| !group.selectable.iter().any(|s| s == a));
        let mut copy = col().grow(1.0).min_w(0.0).gap(2.0).child(caption(&group.host, t));
        if let [only] = group.selectable.as_slice() {
            copy = copy.child(small(format!("Signed in as {}", login(only, revealed)), t));
        }
        let (k, h) = (kind.to_owned(), group.host.clone());
        let mut block = col().w_full().gap(8.0).child(
            row().w_full().gap(12.0).center_y().child(copy).child(
                crate::kit::Switch::new(enabled)
                    .label(format!("Use {label} on {}", group.host))
                    .hide_label()
                    .on_toggle(move |s: &mut App| set_host(s, &k, &h, Some(!enabled), None))
                    .build(&crate::kit::theme(t)),
            ),
        );
        if group.selectable.len() > 1 {
            let active = format!(
                "Active {} account{}",
                cr.cli,
                group.active.as_deref().map(|a| format!(" ({})", login(a, revealed))).unwrap_or_default()
            );
            let mut options = vec![(json!(ACTIVE_ACCOUNT), active)];
            options.extend(group.selectable.iter().map(|a| (json!(a), login(a, revealed))));
            let key = format!("schost::{kind}::account::{}", group.host);
            let select = c::select(s, t, &key, &format!("{label} account for {}", group.host), &options, c::select_width(&key));
            block = block.child(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .child(small("Account", t).grow(1.0))
                    .child(if enabled { select } else { select.disabled(true).opacity(0.64) }),
            );
        }
        if stale {
            let (k, h) = (kind.to_owned(), group.host.clone());
            block = block.child(
                row()
                    .w_full()
                    .gap(12.0)
                    .center_y()
                    .child(
                        text(format!("The chosen login is no longer signed in, so the active {} login is used.", cr.cli))
                            .font(theme::TEXT_XS.lh(18.0))
                            .color(t.c.warning)
                            .wrap()
                            .grow(1.0)
                            .min_w(0.0),
                    )
                    .child(
                        crate::kit::Button::new("Use active login")
                            .variant("outline")
                            .size("xs")
                            .on_press(move |s: &mut App| set_host(s, &k, &h, None, Some(None)))
                            .build(t),
                    ),
            );
        }
        for (account, error) in &group.broken {
            let reason = error.clone().unwrap_or_else(|| format!("{} reports this login as invalid.", cr.cli));
            block = block.child(
                text(format!("{} can't be used: {reason}", login(account, revealed)))
                    .font(theme::TEXT_XS.lh(18.0))
                    .color(t.c.muted_foreground.alpha_mul(0.7))
                    .wrap(),
            );
        }
        if let Some(env) = &group.env {
            block = block.child(
                text(format!("{env} is set on the server, so it overrides the account chosen here until it is unset."))
                    .font(theme::TEXT_XS.lh(18.0))
                    .color(t.c.warning)
                    .wrap(),
            );
        }
        body = body.child(block);
    }
    body
}

fn rescan(pending: bool, t: &T) -> El {
    crate::kit::Button::new("Rescan server environment")
        .variant("ghost-muted")
        .size("icon-xs")
        .icon(&icons::REFRESH_CW)
        .disabled(pending)
        .on_press(scan)
        .build(t)
}

/// The scan sections, in web order after Repositories.
pub fn view(s: &App, t: &T) -> Vec<El> {
    let pending = s.ui.settings.pending.values().any(|m| m == DISCOVERY);
    let Some(result) = s.ui.settings.results.get(DISCOVERY) else {
        let body = text(if pending { "Scanning the server environment…" } else { "Connect an environment to inspect its version control tools and hosting integrations." })
            .font(theme::TEXT_SM)
            .color(t.c.muted_foreground)
            .px(16.0)
            .py(12.0);
        return vec![c::section_with_actions("Version Control", vec![rescan(pending, t)], vec![body], t)];
    };
    let rows = |key: &str| -> Vec<El> {
        result[key].as_array().into_iter().flatten().map(|item| item_row(s, item, t)).collect()
    };
    let (vcs, hosts) = (rows("versionControlSystems"), rows("sourceControlProviders"));
    let mut out = Vec::new();
    let has_vcs = !vcs.is_empty();
    if has_vcs {
        out.push(c::section_with_actions("Version Control", vec![rescan(pending, t)], vcs, t));
    }
    if !hosts.is_empty() {
        let actions = if has_vcs { vec![] } else { vec![rescan(pending, t)] };
        out.push(c::section_with_actions("Source control providers", actions, hosts, t));
    }
    if out.is_empty() {
        let empty = col()
            .w_full()
            .padding(24.0)
            .gap(8.0)
            .items_center()
            .child(text("Nothing detected yet").font(theme::SM_MEDIUM).color(t.c.foreground))
            .child(
                text("Install Git on the server, add optional hosting integrations or credentials your workspace needs, then rescan.")
                    .font(theme::TEXT_SM)
                    .color(t.c.muted_foreground)
                    .wrap(),
            )
            .child(
                crate::kit::Button::new("Scan")
                    .variant("outline")
                    .size("compact")
                    .disabled(pending)
                    .on_press(scan)
                    .build(t),
            );
        out.push(c::section("Server environment", vec![empty], t));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn device_flow_replies_settle_only_the_current_attempt() {
        let prompt = json!({"userCode": "ABCD-1234", "verificationUri": "https://github.com/login/device", "expiresAt": 1762603500000_i64});
        let waiting = app_reply(&AppSignIn::Starting(7), 7, Ok(prompt.clone())).unwrap();
        assert_eq!(waiting, AppSignIn::Waiting { code: "ABCD-1234".into(), uri: "https://github.com/login/device".into(), expires_at: 1762603500000, wait: 0 });
        assert_eq!(app_reply(&AppSignIn::Starting(8), 7, Ok(prompt)), None);
        let pending = AppSignIn::Waiting { code: "ABCD-1234".into(), uri: String::new(), expires_at: 0, wait: 9 };
        assert_eq!(app_reply(&pending, 9, Ok(json!({"account": "octocat"}))), Some(AppSignIn::Idle));
        let denied = json!([{"_tag": "Fail", "error": {"_tag": "SourceControlSignInError", "reason": "denied", "detail": "The sign-in was denied on GitHub."}}]);
        assert_eq!(app_reply(&pending, 9, Err(denied)), Some(AppSignIn::Failed("The sign-in was denied on GitHub.".into())));
        assert_eq!(app_reply(&AppSignIn::Idle, 9, Ok(json!({}))), None);
    }
    #[test]
    fn summaries_follow_web_item_summary() {
        let git = json!({"kind":"git","label":"Git","implemented":true,"status":"available","installHint":"Install Git"});
        assert_eq!(summary(&git), "Available");
        let jj = json!({"kind":"jj","label":"Jujutsu","implemented":false,"status":"missing","installHint":"x"});
        assert_eq!(summary(&jj), "Support for Jujutsu is coming soon.");
        let gh = json!({"kind":"github","label":"GitHub","executable":"gh","status":"available","installHint":"Install gh",
            "auth":{"status":"authenticated","account":{"_tag":"Some","value":"octo"},"detail":{"_tag":"None"}}});
        assert_eq!(summary(&gh), "Authenticated as octo");
        let signed_out = json!({"kind":"github","label":"GitHub","executable":"gh","status":"available","installHint":"Install gh",
            "auth":{"status":"unauthenticated","account":null,"detail":null}});
        assert!(summary(&signed_out).starts_with("GitHub is not authenticated on this server."));
        let missing = json!({"kind":"gitlab","label":"GitLab","status":"missing","installHint":"Install glab","auth":{"status":"unknown"}});
        assert_eq!(summary(&missing), "Not available on this server: Install glab");
    }

    #[test]
    fn host_accounts_group_like_web_and_unpinned_hosts_drop_out() {
        let accounts = [
            json!({"host":"github.com","account":"octo","authenticated":true,"active":false}),
            json!({"host":"github.com","account":"cat","authenticated":true,"active":true}),
            json!({"host":"github.com","account":"old","authenticated":false,"error":"expired"}),
            json!({"host":"github.com","account":"env","authenticated":true,"environmentVariable":"GH_TOKEN"}),
            json!({"host":"ghe.example","account":"me","authenticated":true}),
        ];
        let groups = group_accounts(&accounts);
        assert_eq!(
            groups[0],
            Group {
                host: "github.com".into(),
                active: Some("cat".into()),
                selectable: vec!["octo".into(), "cat".into()],
                broken: vec![("old".into(), Some("expired".into()))],
                env: Some("GH_TOKEN".into()),
            }
        );
        assert_eq!((groups[1].active.as_deref(), groups.len()), (Some("me"), 2));

        let hosts = json!({"github.com": {"enabled": true, "account": "octo"}, "ghe.example": {"enabled": false}});
        // Pinning keeps the switch; unpinning an enabled host removes it entirely.
        assert_eq!(next_hosts(&hosts, "github.com", None, Some(None)), json!({"ghe.example": {"enabled": false}}));
        assert_eq!(
            next_hosts(&hosts, "ghe.example", Some(true), Some(Some("me".into()))),
            json!({"github.com": {"enabled": true, "account": "octo"}, "ghe.example": {"enabled": true, "account": "me"}})
        );
        assert_eq!(
            next_hosts(&json!({}), "github.com", Some(false), None),
            json!({"github.com": {"enabled": false}})
        );
        assert_eq!(next_hosts(&hosts, "ghe.example", Some(true), None), json!({"github.com": {"enabled": true, "account": "octo"}}));
    }
}
