//! Web `AddProviderInstanceDialog`: built-in drivers take Provider, Identity and Config steps,
//! ACP Registry agents take Provider, Identity and a post-create Sign in step, and Codex can
//! instead add a managed ChatGPT account (web `AddCodexAccountDialog`).
use super::{providers, request, server_settings};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons, kit};
use rune::{El, TextEdit, col, el, row, text};
use serde_json::{Map, Value, json};
use std::collections::{HashMap, HashSet};

/// The id the accent picker uses while it edits the wizard's draft.
pub const DRAFT_ID: &str = "+add-provider";
const STEPS: [&str; 3] = ["Provider", "Identity", "Config"];
const ACP_STEPS: [&str; 3] = ["Provider", "Identity", "Sign in"];
const LOCAL_STEPS: [&str; 2] = ["Provider", "Identity"];
/// First-class drivers' own ACP agents (`acpRegistryAgentIds`), hidden from registry results.
const NATIVE_ACP: [&str; 8] = ["codex-acp", "claude-acp", "antigravity-acp", "opencode", "opencode-v2", "pi-acp", "grok-build", "cursor"];
const FAQ: &str = "https://github.com/pingdotgg/t3code/blob/main/docs/user/faq.md#can-i-add-a-bunch-of-accounts-for-one-provider";

/// The ACP Registry choice: a prepared registry agent, the manual form, or a local command.
#[derive(Clone, Debug, PartialEq)]
pub enum Acp {
    Registry(Value),
    Manual,
    Local,
}

/// `providerClients` without ACP Registry; `featured: false` drivers sit under Show more.
const DRIVERS: &[(&str, &str, bool, Option<&str>)] = &[
    ("codex", "Codex", true, None),
    ("claudeAgent", "Claude", true, None),
    ("cursor", "Cursor", true, None),
    ("grok", "Grok", true, None),
    ("fx", "fx", false, None),
    ("opencode", "OpenCode", true, None),
    ("antigravity", "Antigravity", true, None),
    ("kiro", "Kiro", false, Some("Early Access")),
    ("muse", "Muse Code", true, None),
    ("pi", "Pi", true, None),
];

#[derive(Default)]
pub struct Wizard {
    step: usize,
    driver: Option<&'static str>,
    more: bool,
    /// Per driver, like web's `identityByDriver` / `configByDriver`.
    label: HashMap<&'static str, TextEdit>,
    id: HashMap<&'static str, TextEdit>,
    /// Drivers whose instance id the user typed (web `instanceIdOverride`); others follow the label.
    id_typed: HashSet<&'static str>,
    pub accent: HashMap<&'static str, String>,
    config: HashMap<String, TextEdit>,
    attempted: bool,
    acp: Option<Acp>,
    acp_search: TextEdit,
    acp_agents: Option<Result<Vec<Value>, String>>,
    acp_search_id: Option<u64>,
    /// The `prepareAcpRegistryAgent` in flight and the agent it prepares.
    acp_prepare: Option<(u64, Value)>,
    acp_error: Option<String>,
    /// The ACP instance created on Identity; the Sign in step follows it.
    created: Option<String>,
    chatgpt: Option<ChatGpt>,
}

/// Web `AddCodexAccountDialog`: a managed Codex instance per ChatGPT account.
pub struct ChatGpt {
    name: TextEdit,
    created: Option<String>,
    install_sent: bool,
    auth_sent: bool,
}

pub fn open(s: &mut App) {
    s.ui.settings.add_provider = Wizard::default();
    s.ui.settings.error = None;
    s.ui.settings.provider_picker = true;
}

fn wizard(s: &mut App) -> &mut Wizard {
    &mut s.ui.settings.add_provider
}

pub fn driver(w: &Wizard) -> &'static str {
    if w.acp.is_some() { "acpRegistry" } else { w.driver.unwrap_or("codex") }
}

fn driver_label(driver: &str) -> &'static str {
    if driver == "acpRegistry" {
        return "ACP Registry";
    }
    DRIVERS.iter().find(|d| d.0 == driver).map_or("Codex", |d| d.1)
}

fn step_labels(w: &Wizard) -> &'static [&'static str] {
    match w.acp {
        Some(Acp::Local) => &LOCAL_STEPS[..],
        Some(_) => &ACP_STEPS[..],
        None => &STEPS[..],
    }
}

/// Registry agents first-class drivers already cover are left out, like web's search step.
fn compatible(agents: &[Value]) -> Vec<Value> {
    agents.iter().filter(|a| !NATIVE_ACP.contains(&a["id"].as_str().unwrap_or(""))).cloned().collect()
}

/// Whether a non-local ACP Registry instance already runs this agent (web `logic.ts:57-72`).
fn agent_added(instances: &Value, agent: &str) -> bool {
    instances.as_object().into_iter().flatten().any(|(_, i)| i["driver"] == "acpRegistry" && i["config"]["source"] != "local" && i["config"]["agentId"] == agent)
}

/// The registry agent's icon URL, kept only on the official CDN over https.
fn registry_icon(agent: &Value) -> Option<String> {
    let url = agent["icon"].as_str()?;
    let rest = url.strip_prefix("https://cdn.agentclientprotocol.com/")?;
    (!rest.is_empty()).then(|| url.to_owned())
}

/// `slugifyLabel` + `deriveInstanceId`.
fn derive_id(driver: &str, label: &str) -> String {
    let mut slug = String::new();
    for c in label.trim().to_lowercase().chars() {
        if c.is_ascii_lowercase() || c.is_ascii_digit() {
            slug.push(c);
        } else if !slug.ends_with('_') {
            slug.push('_');
        }
    }
    let slug: String = slug.trim_matches('_').chars().take(48).collect();
    if slug.is_empty() { String::new() } else { format!("{driver}_{slug}") }
}

/// `deriveAvailableInstanceId`: the label's id, suffixed `_2`, `_3`… past taken ones. The
/// driver's default label (or none) keeps the driver's own slot id.
fn available_id(driver: &str, label: &str, existing: &[String]) -> String {
    let base = match driver {
        "acpRegistry" => Some(derive_id(driver, label)).filter(|id| !id.is_empty()).unwrap_or_else(|| "acpRegistry_custom".into()),
        _ if label.trim().is_empty() || label == driver_label(driver) => driver.to_owned(),
        _ => derive_id(driver, label),
    };
    if base.is_empty() || !existing.contains(&base) {
        return base;
    }
    (2..)
        .map(|n: usize| {
            let suffix = format!("_{n}");
            format!("{}{suffix}", &base[..base.len().min(64 - suffix.len())])
        })
        .find(|candidate| !existing.contains(candidate))
        .unwrap_or(base)
}

fn validate_id(id: &str, existing: &[String]) -> Option<String> {
    if id.is_empty() {
        return Some("Instance ID is required.".into());
    }
    if id.len() > 64 {
        return Some("Instance ID must be 64 characters or fewer.".into());
    }
    let mut chars = id.chars();
    if !chars.next().is_some_and(|c| c.is_ascii_alphabetic()) || !chars.all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') {
        return Some("Instance ID must start with a letter and use only letters, digits, '-', or '_'.".into());
    }
    existing.contains(&id.to_owned()).then(|| format!("An instance named '{id}' already exists."))
}

/// Codex and Claude's default ids are reserved because they run before configuration.
fn existing(s: &App) -> Vec<String> {
    let mut ids = vec!["codex".to_owned(), "claudeAgent".to_owned()];
    ids.extend(server_settings(s)["providerInstances"].as_object().into_iter().flatten().map(|(id, _)| id.clone()));
    ids
}

fn label_text(w: &Wizard) -> String {
    let driver = driver(w);
    w.label.get(driver).map_or_else(|| driver_label(driver).to_owned(), |e| e.text.clone())
}

fn resolved_id(w: &Wizard, existing: &[String]) -> String {
    let driver = driver(w);
    match w.id.get(driver).filter(|_| w.id_typed.contains(driver)) {
        Some(edit) => edit.text.clone(),
        None => available_id(driver, &label_text(w), existing),
    }
}

/// The ID field's model, re-derived from the label while untyped. A focused one is left alone:
/// rune reads it mid-keystroke, before `on_change` marks the ID typed.
fn id_entry<'a>(w: &'a mut Wizard, driver: &'static str, derived: String) -> &'a mut TextEdit {
    let typed = w.id_typed.contains(driver);
    let edit = w.id.entry(driver).or_insert_with(|| TextEdit::new(derived.clone()));
    if !typed && !edit.focused && edit.text != derived {
        *edit = TextEdit::new(derived);
    }
    edit
}

fn instance_id(s: &App) -> String {
    resolved_id(&s.ui.settings.add_provider, &existing(s))
}

fn config_key(driver: &str, field: &str) -> String {
    format!("{driver}::{field}")
}

/// What stops an ACP choice leaving the Provider step.
fn acp_problem(w: &Wizard) -> Option<&'static str> {
    let field = |f: &str| w.config.get(&config_key("acpRegistry", f)).is_some_and(|e| !e.text.trim().is_empty());
    match w.acp {
        Some(Acp::Local) if !field("commandPath") => Some("Executable is required."),
        Some(Acp::Manual) if !field("agentId") => Some("Select an ACP or configure one manually."),
        _ => None,
    }
}

/// `resolveWizardNavigation`: moving past Identity needs a valid instance id; an ACP instance's
/// last step creates it first.
fn navigate(s: &mut App, step: usize) {
    let error = validate_id(&instance_id(s), &existing(s));
    let w = wizard(s);
    if w.created.is_some() {
        return;
    }
    let step = step.min(step_labels(w).len() - 1);
    if step > 0 && let Some(problem) = acp_problem(w) {
        w.acp_error = Some(problem.into());
        w.step = 0;
        return;
    }
    if w.step <= 1 && step > 1 && error.is_some() {
        w.attempted = true;
        w.step = 1;
        return;
    }
    if step == 2 && matches!(w.acp, Some(Acp::Registry(_) | Acp::Manual)) {
        return save(s);
    }
    w.step = step;
}

/// Picks an ACP choice with its default label, as web's `handleAcpPrepared` and the
/// manual/local buttons do.
fn choose_acp(w: &mut Wizard, acp: Acp) {
    let label = match &acp {
        Acp::Registry(agent) => agent["name"].as_str().unwrap_or("").to_owned(),
        Acp::Local => "Local ACP".into(),
        Acp::Manual => String::new(),
    };
    w.label.insert("acpRegistry", TextEdit::new(label));
    w.id.remove("acpRegistry");
    w.id_typed.remove("acpRegistry");
    w.attempted = false;
    w.acp_error = None;
    w.acp = Some(acp);
}

fn save(s: &mut App) {
    let id = instance_id(s);
    let existing = existing(s);
    let w = wizard(s);
    w.attempted = true;
    if validate_id(&id, &existing).is_some() {
        w.step = 1;
        return;
    }
    let driver = driver(w);
    let label = label_text(w).trim().to_owned();
    let mut config = Map::new();
    match &w.acp {
        Some(Acp::Registry(agent)) => {
            config.insert("agentId".into(), agent["id"].clone());
            config.insert("distribution".into(), json!("auto"));
            if let Some(icon) = registry_icon(agent) {
                config.insert("registryIconUrl".into(), json!(icon));
            }
        }
        Some(Acp::Local) => {
            config.insert("source".into(), json!("local"));
            config.insert("commandArgs".into(), json!([]));
        }
        _ => {}
    }
    let shape = if w.acp == Some(Acp::Local) { json!({"source": "local"}) } else { Value::Null };
    let fields = if matches!(w.acp, Some(Acp::Registry(_))) { vec![] } else { providers::runtime_fields(driver, &shape) };
    for (field, ..) in fields {
        if let Some(value) = w.config.get(&config_key(driver, field)).map(|e| e.text.trim().to_owned()).filter(|v| !v.is_empty()) {
            config.insert(field.into(), json!(value));
        }
    }
    if driver == "codex" {
        config.insert("setupMode".into(), json!("existing"));
    }
    let mut instance = json!({"driver": driver, "enabled": true});
    if !label.is_empty() {
        instance["displayName"] = json!(label);
    }
    if let Some(accent) = w.accent.get(driver).and_then(|a| super::accent::normalize(a)) {
        instance["accentColor"] = json!(accent);
    }
    if !config.is_empty() {
        instance["config"] = Value::Object(config);
    }
    let signs_in = matches!(w.acp, Some(Acp::Registry(_) | Acp::Manual));
    if signs_in {
        w.created = Some(id.clone());
        w.step = 2;
    }
    s.ui.settings.provider = Some(id.clone());
    request(s, "server.updateSettings", json!({"patch": {}, "providerInstanceMutation": {"operation": "create", "instanceId": id, "instance": instance}}));
    if signs_in {
        return;
    }
    s.ui.settings.provider_picker = false;
    let description = format!("{} instance '{id}' was added.", driver_label(driver));
    crate::shell::palette::toast(s, "Provider instance added", Some(&description));
}

fn steps(s: &App, t: &T) -> El {
    let w = &s.ui.settings.add_provider;
    let mut list = row().w_full().gap(4.0).padding(4.0).radius(12.0).fill(t.c.background).border(1.0, t.c.foreground.alpha_mul(0.05)).role("list").label("Setup progress");
    let locked = w.created.is_some();
    for (index, step) in step_labels(w).iter().enumerate() {
        let current = index == w.step;
        let (badge_fill, badge_text, ring) = if index < w.step {
            (t.c.primary, t.c.primary_foreground, t.c.primary)
        } else if current {
            (t.c.primary.alpha_mul(0.1), t.c.primary, t.c.primary.alpha_mul(0.3))
        } else {
            (t.c.card, t.c.muted_foreground, t.c.foreground.alpha_mul(0.1))
        };
        let badge = el().size(20.0, 20.0).radius(10.0).fill(badge_fill).border(1.0, ring).center().child(if index < w.step {
            rune::icon(&icons::CHECK, 16.0).color(badge_text)
        } else {
            text((index + 1).to_string()).font(theme::SM_MEDIUM).color(badge_text)
        });
        let mut item = row()
            .key(format!("add-provider-step-{index}"))
            .grow(1.0)
            .min_w(0.0)
            .gap(8.0)
            .px(10.0)
            .py(8.0)
            .radius(8.0)
            .center_y()
            .role("button")
            .label(format!("{step}, step {}", index + 1))
            .disabled(locked)
            .cursor(if locked { rune::Cursor::Default } else { rune::Cursor::Pointer })
            .on_press(move |s: &mut App| navigate(s, index))
            .child(badge)
            .child(text(*step).font(theme::SM_MEDIUM).color(if current { t.c.foreground } else { t.c.muted_foreground }).truncate());
        item = if current {
            item.fill(t.c.card).border(1.0, t.c.foreground.alpha_mul(0.05))
        } else if locked {
            item
        } else {
            item.hover(t.c.card)
        };
        list = list.child(item);
    }
    list
}

fn option_grid(s: &App, t: &T, featured: bool) -> El {
    let selected = driver(&s.ui.settings.add_provider);
    let mut cards = vec![];
    for &(kind, label, _, badge) in DRIVERS.iter().filter(|d| d.2 == featured) {
        let checked = kind == selected;
        let mut option = row()
            .key(format!("add-provider-{kind}"))
            .w_full()
            .gap(12.0)
            .px(12.0)
            .py(12.0)
            .radius(8.0)
            .center_y()
            .role("radio")
            .label(label)
            .checked(checked)
            .cursor(rune::Cursor::Pointer)
            .on_press(move |s: &mut App| {
                let w = wizard(s);
                w.driver = Some(kind);
                w.acp = None;
                w.acp_error = None;
                w.attempted = false;
            })
            .child(crate::chat::composer::provider_icon(t, kind, 16.0))
            .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground).grow(1.0).min_w(0.0).truncate());
        if checked {
            option = option
                .fill(t.c.primary.alpha_mul(if t.dark { 0.15 } else { 0.08 }))
                .border(2.0, t.c.primary)
                .child(el().size(20.0, 20.0).radius(10.0).fill(t.c.primary).center().child(rune::icon(&icons::CHECK, 14.0).color(t.c.primary_foreground)));
        } else {
            option = option
                .fill(if t.dark { t.c.foreground.alpha_mul(0.03) } else { t.c.card })
                .border(1.0, if t.dark { t.c.foreground.alpha_mul(0.05) } else { rune::Color::BLACK.alpha_mul(0.05) })
                .hover(t.c.muted);
        }
        if let Some(badge) = badge {
            option = option.child(kit::badge(badge, "warning", "sm", t));
        }
        cards.push(option);
    }
    // Web's `grid sm:grid-cols-2` inside a `gap-3` radio group: two equal columns.
    let mut grid = col().w_full().gap(12.0).role("radiogroup");
    let mut cards = cards.into_iter();
    while let Some(first) = cards.next() {
        let mut pair = row().w_full().child(row().w_frac(0.5).pr(6.0).child(first));
        if let Some(second) = cards.next() {
            pair = pair.child(row().w_frac(0.5).pl(6.0).child(second));
        }
        grid = grid.child(pair);
    }
    grid
}

/// A `SettingsRow` in the wizard's plain group: copy on the left, the control on the right.
fn field_row(t: &T, title: &str, description: &str, error: Option<String>, control: El) -> El {
    let mut copy = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text(description.to_owned()).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap());
    if let Some(error) = error {
        copy = copy.child(text(error).font(theme::TEXT_XS.lh(18.0)).color(t.c.destructive).wrap().role("alert"));
    }
    row().w_full().py(12.0).gap(32.0).center_y().child(copy).child(control)
}

fn input(t: &T, key: String, edit: &TextEdit, placeholder: &str, edit_of: impl for<'a> Fn(&'a mut App) -> &'a mut TextEdit + 'static) -> El {
    kit::Input::new(edit).size("sm").placeholder(placeholder.to_owned()).on_edit(edit_of).build(t).key(key).w(224.0).single_line()
}

fn identity(s: &App, t: &T) -> El {
    let w = &s.ui.settings.add_provider;
    let driver = driver(w);
    let label = TextEdit::new(label_text(w));
    let label_edit = w.label.get(driver).cloned().unwrap_or(label);
    let id = instance_id(s);
    let id_edit = w.id.get(driver).filter(|e| w.id_typed.contains(driver) || e.text == id).cloned().unwrap_or_else(|| TextEdit::new(id.clone()));
    let error = validate_id(&id, &existing(s)).filter(|_| w.attempted);
    col()
        .w_full()
        .child(field_row(
            t,
            "Label",
            "Shown in the provider list.",
            None,
            input(t, format!("add-provider-label-{driver}"), &label_edit, "e.g. Work", move |s: &mut App| {
                let initial = label_text(&s.ui.settings.add_provider);
                wizard(s).label.entry(driver).or_insert_with(|| TextEdit::new(initial))
            })
            .label("Label"),
        ))
        .child(field_row(
            t,
            "Instance ID",
            "Letters, digits, '-', or '_'.",
            error,
            kit::Input::new(&id_edit)
                .size("sm")
                .placeholder(format!("{driver}_work"))
                .on_edit(move |s: &mut App| {
                    let derived = instance_id(s);
                    id_entry(wizard(s), driver, derived)
                })
                .on_change(move |s: &mut App, _| {
                    wizard(s).id_typed.insert(driver);
                })
                .build(t)
                .key(format!("add-provider-id-{driver}"))
                .w(224.0)
                .single_line()
                .label("Instance ID"),
        ))
        .child(field_row(
            t,
            "Accent color",
            "Optional marker shown in the picker.",
            None,
            super::accent::trigger(s, t, DRAFT_ID, &label_text(w)),
        ))
}

fn config(s: &App, t: &T) -> El {
    let w = &s.ui.settings.add_provider;
    let driver = driver(w);
    let fields = providers::runtime_fields(driver, &Value::Null);
    if fields.is_empty() {
        return text("This driver has no required configuration. You can add the instance now.").font(theme::TEXT_SM).color(t.c.muted_foreground).wrap();
    }
    let mut body = col().w_full();
    for (field, title, description, placeholder) in fields {
        let key = config_key(driver, field);
        let edit = w.config.get(&key).cloned().unwrap_or_default();
        let owned = key.clone();
        let mut control = input(t, format!("add-provider-config-{key}"), &edit, placeholder, move |s: &mut App| wizard(s).config.entry(owned.clone()).or_default()).label(title);
        if providers::is_secret_field(driver, field) {
            control = control.secure();
        }
        body = body.child(field_row(t, title, description, None, control));
    }
    body
}

fn environment(s: &App) -> &str {
    s.state().config.as_ref().and_then(|c| c["environment"]["label"].as_str()).unwrap_or("this environment")
}

/// Web `MultipleAccountsNote`, for Codex and Claude once one of that driver is signed in.
fn multiple_accounts(s: &App, t: &T, driver: &str) -> Option<El> {
    let name = match driver {
        "codex" => "Codex",
        "claudeAgent" => "Claude",
        _ => return None,
    };
    crate::chat::composer::providers(s).iter().any(|p| p["driver"] == driver && p["auth"]["status"] == "authenticated").then(|| {
        row()
            .w_full()
            .gap(8.0)
            .px(12.0)
            .py(10.0)
            .radius(8.0)
            .border(1.0, t.c.border)
            .fill(t.c.card)
            .role("alert")
            .child(rune::icon(&icons::INFO, 16.0).color(t.c.foreground))
            .child(text(format!("We do not recommend managing multiple {name} accounts in T3 Code.")).font(theme::TEXT_SM).color(t.c.foreground).wrap().grow(1.0).min_w(0.0))
            .child(
                text("More info")
                    .font(theme::SM_MEDIUM)
                    .color(t.c.primary)
                    .role("link")
                    .cursor(rune::Cursor::Pointer)
                    .on_press(|s: &mut App| super::auth::open_url(s, FAQ)),
            )
    })
}

fn notice(t: &T, message: &str) -> El {
    text(message.to_owned()).font(theme::TEXT_SM).color(t.c.destructive).wrap().role("alert")
}

/// Web `AcpRegistrySearchStep`: the registry list, filtered as the user types.
fn acp_search(s: &App, t: &T) -> El {
    let w = &s.ui.settings.add_provider;
    let preparing = w.acp_prepare.as_ref().map(|(_, agent)| agent["id"].clone());
    let busy = preparing.is_some();
    let mut out = col().w_full().gap(12.0).child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(
                kit::Input::new(&w.acp_search)
                    .size("sm")
                    .placeholder("Search agents…")
                    .disabled(busy)
                    .on_edit(|s: &mut App| &mut wizard(s).acp_search)
                    .build(t)
                    .single_line()
                    .grow(1.0)
                    .label("Search ACP Registry"),
            )
            .child(kit::Button::new("Enter manually").variant("ghost-muted").size("sm").disabled(busy).on_press(|s: &mut App| choose_acp(wizard(s), Acp::Manual)).build(t))
            .child(kit::Button::new("Local ACP command").variant("outline").size("sm").disabled(busy).on_press(|s: &mut App| choose_acp(wizard(s), Acp::Local)).build(t)),
    );
    if let Some(error) = &w.acp_error {
        out = out.child(notice(t, error));
    }
    let agents = match &w.acp_agents {
        None => return out.child(text("Searching the registry...").font(theme::TEXT_SM).color(t.c.muted_foreground)),
        Some(Err(error)) => return out.child(notice(t, error)),
        Some(Ok(agents)) => agents,
    };
    let query = w.acp_search.text.trim().to_lowercase();
    let shown: Vec<&Value> = agents
        .iter()
        .filter(|a| query.is_empty() || ["id", "name", "description"].iter().any(|k| a[*k].as_str().is_some_and(|v| v.to_lowercase().contains(&query))))
        .collect();
    if shown.is_empty() {
        return out.child(
            col()
                .w_full()
                .py(16.0)
                .gap(4.0)
                .items_center()
                .child(text("No compatible agents found").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text("Try a broader search.").font(theme::TEXT_XS).color(t.c.muted_foreground)),
        );
    }
    let instances = &server_settings(s)["providerInstances"];
    let mut list = col().w_full().gap(2.0);
    for agent in shown {
        let id = agent["id"].as_str().unwrap_or("").to_owned();
        let name = agent["name"].as_str().unwrap_or(&id).to_owned();
        let mut item = row()
            .key(format!("acp-agent-{id}"))
            .w_full()
            .gap(12.0)
            .px(8.0)
            .py(8.0)
            .radius(8.0)
            .center_y()
            .child(crate::chat::composer::provider_icon(t, "acpRegistry", 16.0))
            .child(
                col()
                    .grow(1.0)
                    .min_w(0.0)
                    .gap(2.0)
                    .child(text(name.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
                    .child(text(agent["description"].as_str().unwrap_or("").to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate()),
            );
        if let Some(site) = agent["website"].as_str().or(agent["repository"].as_str()).map(str::to_owned) {
            item = item.child(
                kit::Button::new(format!("About {name}"))
                    .variant("ghost-muted")
                    .size("icon-sm")
                    .icon(&icons::EXTERNAL_LINK)
                    .on_press(move |s: &mut App| super::auth::open_url(s, &site))
                    .build(t),
            );
        }
        let added = agent_added(instances, &id);
        let label = if added {
            "Added"
        } else if preparing.as_ref().is_some_and(|p| p == &agent["id"]) {
            if agent["distribution"] == "binary" { "Downloading" } else { "Preparing" }
        } else {
            "Add"
        };
        let owned = agent.clone();
        item = item.child(
            kit::Button::new(label)
                .variant("outline")
                .size("sm")
                .disabled(added || busy)
                .on_press(move |s: &mut App| {
                    if let Some(net) = s.src.net() {
                        let pending = net.request("server.prepareAcpRegistryAgent", json!({"agentId": owned["id"]}));
                        let w = wizard(s);
                        w.acp_prepare = Some((pending, owned.clone()));
                        w.acp_error = None;
                    }
                })
                .build(t),
        );
        list = list.child(item);
    }
    out.child(list.key("acp-registry-results").max_h(256.0).scroll(rune::ScrollSpec { stick_bottom: false }))
}

/// The manual and local ACP forms that replace the search on the Provider step.
fn acp_form(s: &App, t: &T, local: bool) -> El {
    let w = &s.ui.settings.add_provider;
    let (title, description) = if local {
        ("Local ACP command", format!("Run an installed ACP executable on {}.", environment(s)))
    } else {
        ("Enter manually", "Enter an official registry ID and any local executable or auth override.".to_owned())
    };
    let mut body = col().w_full().gap(4.0).child(
        row()
            .w_full()
            .gap(8.0)
            .center_y()
            .child(col().grow(1.0).min_w(0.0).gap(2.0).child(text(title).font(theme::SM_MEDIUM).color(t.c.foreground)).child(text(description).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()))
            .child(
                kit::Button::new("Search registry")
                    .variant("ghost-muted")
                    .size("xs")
                    .icon(&icons::ARROW_LEFT)
                    .on_press(|s: &mut App| {
                        let w = wizard(s);
                        w.acp = None;
                        w.acp_error = None;
                    })
                    .build(t),
            ),
    );
    if let Some(error) = &w.acp_error {
        body = body.child(notice(t, error));
    }
    let shape = if local { json!({"source": "local"}) } else { Value::Null };
    for (field, title, description, placeholder) in providers::runtime_fields("acpRegistry", &shape) {
        let key = config_key("acpRegistry", field);
        let edit = w.config.get(&key).cloned().unwrap_or_default();
        let owned = key.clone();
        let control = input(t, format!("add-provider-config-{key}"), &edit, placeholder, move |s: &mut App| wizard(s).config.entry(owned.clone()).or_default()).label(title);
        body = body.child(field_row(t, title, description, None, control));
    }
    body
}

/// Step 1's header for a registry agent: name, version and distribution, and its links.
fn agent_header(t: &T, agent: &Value) -> El {
    let mut links = row().gap(4.0);
    for (label, key) in [("Docs", "website"), ("Source", "repository")] {
        if let Some(url) = agent[key].as_str().map(str::to_owned) {
            links = links.child(kit::Button::new(label).variant("ghost-muted").size("xs").icon(&icons::EXTERNAL_LINK).on_press(move |s: &mut App| super::auth::open_url(s, &url)).build(t));
        }
    }
    row()
        .w_full()
        .gap(12.0)
        .pb(8.0)
        .center_y()
        .child(crate::chat::composer::provider_icon(t, "acpRegistry", 20.0))
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(2.0)
                .child(text(agent["name"].as_str().unwrap_or("").to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text(format!("v{} · {}", agent["version"].as_str().unwrap_or("?"), agent["distribution"].as_str().unwrap_or("?"))).font(theme::TEXT_XS).color(t.c.muted_foreground)),
        )
        .child(links)
}

/// Whether the created instance is signed in, and whether a sign-in is running.
fn sign_in_state(s: &App, id: &str) -> (bool, bool) {
    let Some(p) = crate::chat::composer::provider(s, id) else { return (false, false) };
    let state = super::resource(s, "provider.auth.subscribe", json!({"instanceId": id}));
    let d = super::auth::derive(p, state);
    (d.signed_in, d.active)
}

/// Web `ProviderWizardAuthenticationStep`.
fn sign_in(s: &App, t: &T, id: &str) -> El {
    let state = super::resource(s, "provider.auth.subscribe", json!({"instanceId": id}));
    let methods = state.and_then(|a| a["methods"].as_array()).map_or(0, Vec::len);
    let live = crate::chat::composer::provider(s, id);
    let (signed_in, _) = sign_in_state(s, id);
    if let Some(p) = live.filter(|_| methods > 0 || signed_in) {
        let mut p = p.clone();
        p["setup"]["canAuthenticate"] = json!(methods > 0);
        return super::auth::view(s, t, &p);
    }
    let error = state.and_then(|a| a.get("error"));
    let discovering = !signed_in && error.is_none() && state.is_none_or(|a| a.get("methods").is_none());
    let description = if discovering {
        "Discovering sign-in methods…".to_owned()
    } else if let Some(error) = error {
        super::failure_detail(error).unwrap_or_else(|| "Could not load sign-in status.".into())
    } else if let Some(message) = state.and_then(|a| a["message"].as_str()) {
        message.to_owned()
    } else {
        "No in-app sign-in advertised. Follow the provider's docs to finish setup.".into()
    };
    let docs = live.and_then(|p| p["setup"]["documentationUrl"].as_str()).map(str::to_owned);
    let control = match docs {
        _ if discovering => kit::Button::new("Sign in").variant("outline").size("sm").disabled(true).build(t),
        Some(url) => kit::Button::new("Open docs").variant("outline").size("sm").on_press(move |s: &mut App| super::auth::open_url(s, &url)).build(t),
        None => el(),
    };
    field_row(t, "Account", &description, None, control)
}

pub fn dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    chatgpt_dialog(s, t, layers);
    if !s.ui.settings.provider_picker {
        return;
    }
    let w = &s.ui.settings.add_provider;
    let environment = environment(s);
    let mut panel = col().w_full().gap(16.0).px(24.0).py(20.0).radius(12.0).fill(t.c.background.alpha_mul(0.8)).border(1.0, t.c.foreground.alpha_mul(0.05));
    let body = match (w.step, &w.created) {
        (_, Some(id)) => sign_in(s, t, id),
        (0, _) => {
            let mut step = col()
                .w_full()
                .gap(8.0)
                .child(text("Provider").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(option_grid(s, t, true));
            if w.acp.is_none()
                && let Some(note) = multiple_accounts(s, t, driver(w))
            {
                step = step.child(note);
            }
            step = step.child(
                row().pt(8.0).child(
                    kit::Button::new(if w.more { "Show less" } else { "Show more" })
                        .variant("ghost-muted")
                        .size("xs")
                        .icon(if w.more { &icons::CHEVRON_UP } else { &icons::CHEVRON_DOWN })
                        .on_press(|s: &mut App| wizard(s).more ^= true)
                        .build(t)
                        .expanded(w.more),
                ),
            );
            if w.more {
                step = step.child(col().w_full().pt(16.0).child(option_grid(s, t, false)));
                let divider = |t: &T| el().h(1.0).grow(1.0).fill(t.c.border);
                step = step.child(
                    row()
                        .w_full()
                        .pt(16.0)
                        .gap(12.0)
                        .center_y()
                        .child(divider(t))
                        .child(text("Or choose from ACP Registry").font(theme::TEXT_XS).color(t.c.muted_foreground))
                        .child(divider(t)),
                );
                step = step.child(match w.acp {
                    Some(Acp::Manual) => acp_form(s, t, false),
                    Some(Acp::Local) => acp_form(s, t, true),
                    _ => acp_search(s, t),
                });
            }
            step
        }
        (1, _) => {
            let mut body = col().w_full();
            if let Some(Acp::Registry(agent)) = &w.acp {
                body = body.child(agent_header(t, agent));
            }
            body.child(identity(s, t))
        }
        _ => config(s, t),
    };
    // The window can be shorter than the open Show more fold; web's `DialogPanel` scrolls too.
    panel = panel.child(col().key("add-provider-body").w_full().max_h(440.0).scroll(rune::ScrollSpec { stick_bottom: false }).child(body));
    if let Some(error) = &s.ui.settings.error {
        panel = panel.child(notice(t, error));
    }
    let step = w.step;
    let mut footer = row().w_full().gap(8.0).child(el().grow(1.0));
    if let Some(id) = w.created.clone() {
        let (signed_in, active) = sign_in_state(s, &id);
        footer = footer.child(
            kit::Button::new(if signed_in { "Done" } else { "Skip for now" })
                .variant(if signed_in { "default" } else { "outline" })
                .size("sm")
                .disabled(active)
                .on_press(|s: &mut App| s.ui.settings.provider_picker = false)
                .build(t),
        );
    } else {
        footer = footer.child(
            kit::Button::new(if step == 0 { "Cancel" } else { "Back" })
                .variant(if step == 0 { "ghost-muted" } else { "outline" })
                .size("sm")
                .on_press(move |s: &mut App| {
                    if step == 0 {
                        s.ui.settings.provider_picker = false;
                    } else {
                        wizard(s).step = step - 1;
                    }
                })
                .build(t),
        );
        let last = step + 1 == step_labels(w).len();
        footer = match step {
            0 if w.acp.is_none() && driver(w) == "codex" => footer
                .child(kit::Button::new("Configure manually").variant("outline").size("sm").on_press(|s: &mut App| navigate(s, 1)).build(t))
                .child(
                    kit::Button::new("Continue with ChatGPT")
                        .size("sm")
                        .on_press(|s: &mut App| {
                            s.ui.settings.provider_picker = false;
                            s.ui.settings.error = None;
                            wizard(s).chatgpt = Some(ChatGpt { name: TextEdit::new("Personal"), created: None, install_sent: false, auth_sent: false });
                        })
                        .build(t),
                ),
            1 if matches!(w.acp, Some(Acp::Registry(_) | Acp::Manual)) => {
                footer.child(kit::Button::new("Continue to sign-in").size("sm").on_press(move |s: &mut App| navigate(s, 2)).build(t))
            }
            _ if last => footer.child(kit::Button::new("Add instance").size("sm").on_press(save).build(t)),
            _ => footer.child(kit::Button::new("Next").size("sm").on_press(move |s: &mut App| navigate(s, step + 1)).build(t)),
        };
    }
    kit::Dialog::new("add-provider", "Add provider")
        .open(true)
        .width(768.0)
        .description(format!("Add an account or configure a provider on {environment}."))
        .on_close(|s: &mut App| {
            super::accent::close(s);
            s.ui.settings.provider_picker = false;
        })
        .child(steps(s, t))
        .child(panel)
        .child(footer)
        .build(t, layers);
}

/// Creates the managed Codex instance; `poll` then installs it if needed and starts ChatGPT sign-in.
fn add_chatgpt(s: &mut App) {
    let Some(chatgpt) = &wizard(s).chatgpt else { return };
    let name = chatgpt.name.text.trim().to_owned();
    if name.is_empty() || chatgpt.created.is_some() {
        return;
    }
    let id = format!("codex_{}", s5_net::uuid().replace('-', ""));
    let instance = json!({"driver": "codex", "displayName": format!("ChatGPT - {name}"), "enabled": true, "config": {"enabled": true, "setupMode": "managed"}});
    if let Some(chatgpt) = &mut wizard(s).chatgpt {
        chatgpt.created = Some(id.clone());
    }
    s.ui.settings.provider = Some(id.clone());
    request(s, "server.updateSettings", json!({"patch": {}, "providerInstanceMutation": {"operation": "create", "instanceId": id, "instance": instance}}));
}

fn chatgpt_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(chatgpt) = &s.ui.settings.add_provider.chatgpt else { return };
    let close = |s: &mut App| wizard(s).chatgpt = None;
    let live = chatgpt.created.as_deref().and_then(|id| crate::chat::composer::provider(s, id));
    let (title, mut body, footer) = match &chatgpt.created {
        None => {
            let empty = chatgpt.name.text.trim().is_empty();
            let mut body = col().w_full().gap(12.0);
            if let Some(note) = multiple_accounts(s, t, "codex") {
                body = body.child(note);
            }
            // Web's stacked `Field`: label, input, then description.
            let name = kit::Input::new(&chatgpt.name)
                .placeholder("e.g. Personal or Work")
                .on_edit(|s: &mut App| &mut wizard(s).chatgpt.get_or_insert_with(|| ChatGpt { name: TextEdit::new(""), created: None, install_sent: false, auth_sent: false }).name)
                .on_submit(|s: &mut App, _| add_chatgpt(s))
                .build(t)
                .key("chatgpt-account-name")
                .w_full()
                .single_line()
                .label("Account name");
            body = body.child(
                col()
                    .w_full()
                    .gap(8.0)
                    .child(text("Account name").font(theme::SM_MEDIUM).color(t.c.foreground))
                    .child(name)
                    .child(text("Shown in the provider list and model picker.").font(theme::TEXT_XS).color(t.c.muted_foreground)),
            );
            let footer = row()
                .w_full()
                .gap(8.0)
                .child(el().grow(1.0))
                .child(kit::Button::new("Cancel").variant("outline").size("sm").on_press(close).build(t))
                .child(kit::Button::new("Continue").size("sm").disabled(empty).on_press(add_chatgpt).build(t));
            ("Add ChatGPT account".to_owned(), body, footer)
        }
        Some(_) => {
            let name = live.and_then(|p| p["displayName"].as_str()).map_or_else(|| format!("ChatGPT - {}", chatgpt.name.text.trim()), str::to_owned);
            let body = match live.filter(|p| p["setup"].is_object()) {
                Some(p) => col().w_full().child(super::auth::view(s, t, p)),
                None => col().w_full().child(text("Preparing managed setup.").font(theme::TEXT_SM).color(t.c.muted_foreground)),
            };
            let footer = row().w_full().child(el().grow(1.0)).child(kit::Button::new("Finish later").variant("outline").size("sm").on_press(close).build(t));
            (name, body, footer)
        }
    };
    if let Some(error) = &s.ui.settings.error {
        body = body.child(notice(t, error));
    }
    kit::Dialog::new("add-chatgpt-account", title)
        .open(true)
        .width(512.0)
        .description("Each account has its own Codex instance and sign-in. Choose the other account on the sign-in page.")
        .on_close(close)
        .child(body)
        .child(footer)
        .build(t, layers);
}

/// Resources the wizard needs: the created instance's sign-in state, and a new ChatGPT
/// account's install and sign-in state.
pub fn wanted(s: &App) -> Vec<(String, Value)> {
    let w = &s.ui.settings.add_provider;
    let live = |id: &&String| crate::chat::composer::provider(s, id).is_some();
    let mut out = Vec::new();
    if let Some(id) = w.created.as_ref().filter(|_| s.ui.settings.provider_picker).filter(live) {
        out.push(("provider.auth.subscribe".to_owned(), json!({"instanceId": id})));
    }
    if let Some(id) = w.chatgpt.as_ref().and_then(|c| c.created.as_ref()).filter(live) {
        out.push(("provider.auth.subscribe".to_owned(), json!({"instanceId": id})));
        out.push(("provider.install.subscribe".to_owned(), json!({"instanceId": id})));
    }
    out
}

/// Whether an install state has finished with a version (web `isInstallReady`).
fn install_ready(state: Option<&Value>) -> bool {
    state.is_some_and(|i| !matches!(i["phase"].as_str(), Some("downloading" | "extracting" | "verifying" | "failed" | "cancelled")) && !i["installedVersion"].is_null())
}

/// Drives the wizard's own requests: the registry list, agent preparation, and the ChatGPT
/// account's install then sign-in.
pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let w = &s.ui.settings.add_provider;
    if s.ui.settings.provider_picker
        && w.step == 0
        && w.acp_agents.is_none()
        && w.acp_search_id.is_none()
        && let Some(net) = s.src.net()
    {
        let pending = net.request("server.searchAcpRegistry", json!({"query": "", "limit": 512}));
        wizard(s).acp_search_id = Some(pending);
    }
    if let Some(pending) = s.ui.settings.add_provider.acp_search_id
        && let Some(reply) = s.src.net().and_then(|n| n.take_reply(pending))
    {
        let w = wizard(s);
        w.acp_search_id = None;
        w.acp_agents = Some(match reply {
            Ok(result) => Ok(compatible(result["agents"].as_array().map(Vec::as_slice).unwrap_or_default())),
            Err(error) => Err(super::failure_detail(&error).unwrap_or_else(|| "The ACP Registry search failed.".into())),
        });
        changed = true;
    }
    if let Some((pending, agent)) = s.ui.settings.add_provider.acp_prepare.clone()
        && let Some(reply) = s.src.net().and_then(|n| n.take_reply(pending))
    {
        let w = wizard(s);
        w.acp_prepare = None;
        match reply {
            Ok(_) => {
                choose_acp(w, Acp::Registry(agent));
                w.step = 1;
            }
            Err(error) => w.acp_error = Some(super::failure_detail(&error).unwrap_or_else(|| "The ACP could not be prepared.".into())),
        }
        changed = true;
    }
    let Some(id) = s.ui.settings.add_provider.chatgpt.as_ref().and_then(|c| c.created.clone()) else { return changed };
    let Some(p) = crate::chat::composer::provider(s, &id) else { return changed };
    let installed = p["installed"] != false || install_ready(super::resource(s, "provider.install.subscribe", json!({"instanceId": id})));
    let ready = installed && p["setup"].is_object();
    let chatgpt = wizard(s).chatgpt.as_mut().expect("checked above");
    if !installed && !chatgpt.install_sent {
        chatgpt.install_sent = true;
        request(s, "provider.install.start", json!({"instanceId": id}));
        changed = true;
    } else if ready && !chatgpt.auth_sent {
        chatgpt.auth_sent = true;
        request(s, "provider.auth.start", json!({"instanceId": id, "methodId": "chatgpt"}));
        changed = true;
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_ids_derive_from_the_label_and_skip_taken_ones() {
        let existing: Vec<String> = ["codex", "claudeAgent", "codex_work", "codex_work_2"].map(String::from).to_vec();
        assert_eq!(available_id("codex", "Codex", &existing), "codex_2");
        assert_eq!(available_id("grok", "", &existing), "grok");
        assert_eq!(available_id("codex", "  Work!! ", &existing), "codex_work_3");
        assert_eq!(available_id("opencode", "My Team / Prod", &existing), "opencode_my_team_prod");
        assert_eq!(derive_id("codex", "***"), "");
        assert_eq!(validate_id("", &existing).as_deref(), Some("Instance ID is required."));
        assert!(validate_id("1abc", &existing).unwrap().starts_with("Instance ID must start with a letter"));
        assert_eq!(validate_id("codex_work", &existing).as_deref(), Some("An instance named 'codex_work' already exists."));
        assert_eq!(validate_id("codex-work_9", &existing), None);
        assert!(validate_id(&"a".repeat(65), &existing).is_some());

        // The id follows the label until the user types one (web `instanceIdOverride`).
        let mut w = Wizard { driver: Some("opencode"), ..Wizard::default() };
        w.label.insert("opencode", TextEdit::new("Work Team"));
        w.id.insert("opencode", TextEdit::new("opencode"));
        assert_eq!(resolved_id(&w, &existing), "opencode_work_team");
        assert_eq!(id_entry(&mut w, "opencode", "opencode_work_team".into()).text, "opencode_work_team");
        // A keystroke into the focused field survives the accessor read before `on_change`.
        let edit = id_entry(&mut w, "opencode", "opencode_work_team".into());
        *edit = TextEdit::new("9");
        edit.focused = true;
        assert_eq!(id_entry(&mut w, "opencode", "opencode_work_team".into()).text, "9");
        w.id_typed.insert("opencode");
        assert_eq!(resolved_id(&w, &existing), "9");
    }

    #[test]
    fn acp_registry_choices_follow_web_ids_steps_and_filters() {
        let agents = [json!({"id": "devin"}), json!({"id": "codex-acp"}), json!({"id": "cursor"}), json!({"id": "goose"})];
        assert_eq!(compatible(&agents).iter().map(|a| a["id"].as_str().unwrap()).collect::<Vec<_>>(), ["devin", "goose"]);
        let instances = json!({
            "acpRegistry_devin": {"driver": "acpRegistry", "config": {"agentId": "devin"}},
            "acpRegistry_local": {"driver": "acpRegistry", "config": {"source": "local", "agentId": "goose"}},
        });
        assert!(agent_added(&instances, "devin"));
        assert!(!agent_added(&instances, "goose"));
        assert_eq!(registry_icon(&json!({"icon": "https://cdn.agentclientprotocol.com/registry/v1/latest/devin.svg"})).as_deref(), Some("https://cdn.agentclientprotocol.com/registry/v1/latest/devin.svg"));
        assert_eq!(registry_icon(&json!({"icon": "http://cdn.agentclientprotocol.com/x.svg"})), None);
        assert_eq!(registry_icon(&json!({"icon": "https://evil.example/x.svg"})), None);

        let existing: Vec<String> = ["codex", "claudeAgent", "acpRegistry_devin"].map(String::from).to_vec();
        let mut w = Wizard::default();
        choose_acp(&mut w, Acp::Registry(json!({"id": "devin", "name": "Devin"})));
        assert_eq!((driver(&w), step_labels(&w)), ("acpRegistry", &ACP_STEPS[..]));
        assert_eq!(resolved_id(&w, &existing), "acpRegistry_devin_2");
        choose_acp(&mut w, Acp::Manual);
        assert_eq!(resolved_id(&w, &existing), "acpRegistry_custom");
        assert_eq!(acp_problem(&w), Some("Select an ACP or configure one manually."));
        w.config.insert(config_key("acpRegistry", "agentId"), TextEdit::new("devin"));
        assert_eq!(acp_problem(&w), None);
        choose_acp(&mut w, Acp::Local);
        assert_eq!((resolved_id(&w, &existing).as_str(), step_labels(&w).len()), ("acpRegistry_local_acp", 2));
        assert_eq!(acp_problem(&w), Some("Executable is required."));
        assert_eq!(driver_label("acpRegistry"), "ACP Registry");
        assert!(install_ready(Some(&json!({"phase": "succeeded", "installedVersion": "1.0.0"}))));
        assert!(!install_ready(Some(&json!({"phase": "downloading", "installedVersion": "0.9.0"}))));
        assert!(!install_ready(Some(&json!({"phase": "idle", "installedVersion": null}))));
    }
}
