//! The setup wizard (components/onboarding/WelcomeWizard.tsx): Connect, Agents, Projects. It
//! opens once, on a fresh workspace whose client settings have no `onboardingCompletedAt`, and
//! closing it or finishing records that, as `completeOnboarding` does. It sets up the active
//! environment; the web sets up several selected computers at once.

use std::collections::HashSet;

use rune::{El, col, icon, row, text};
use serde_json::{Value, json};

use crate::theme::{self, AlphaMul, T};
use crate::{App, Route, data, icons, kit};

const STAGES: [&str; 3] = ["Connect", "Agents", "Projects"];
/// `PRIMARY_AGENT_DRIVERS`.
const AGENTS: [(&str, &str); 2] = [("codex", "Codex"), ("claudeAgent", "Claude Code")];
const PRIVACY_POLICY_URL: &str = "https://t3.codes/privacy";

#[derive(Default, Clone, Copy, PartialEq, Debug)]
enum Step {
    #[default]
    Connect,
    Agents,
    Projects,
}

#[derive(Default)]
pub struct Ui {
    decided: bool,
    pub open: bool,
    step: Step,
    pairing_open: bool,
    scan: Option<u64>,
    candidates: Option<Vec<Value>>,
    truncated: bool,
    scan_error: Option<String>,
    /// Candidate paths to import.
    selected: HashSet<String>,
    import: Option<Import>,
}

/// One candidate at a time: create its project when it has none, then import its history.
struct Import {
    queue: Vec<(Value, Option<String>)>,
    stage: Option<Stage>,
    first_project: Option<String>,
    imported: u64,
    skipped: u64,
    failed: u64,
}

enum Stage {
    Create(u64, String, String),
    History(u64, String),
}

fn completed(s: &App) -> bool {
    s.client_settings.values.get("onboardingCompletedAt").is_some_and(|v| !v.is_null())
}

/// `isFreshFirstRunWorkspace`: nothing beyond the server's own bootstrap project and thread.
fn fresh(st: &s5_net::State) -> bool {
    st.projects.len() <= 1 && st.threads.len() <= 1
}

fn complete(s: &mut App) {
    s.client_settings.values.insert("onboardingCompletedAt".into(), Value::String(crate::shell::palette::add_project::iso_now()));
    s.client_settings.save();
}

/// `getOnboardingProviderState`.
fn provider_state(p: Option<&Value>) -> &'static str {
    let Some(p) = p else { return "checking" };
    if p["enabled"] == false || p["status"] == "disabled" {
        "disabled"
    } else if p["installed"] == false && p["status"] == "warning" && p["auth"]["status"] == "unknown" {
        "checking"
    } else if p["installed"] == false {
        "install"
    } else if p["auth"]["status"] == "unauthenticated" {
        "signIn"
    } else if p["status"] == "ready" {
        "ready"
    } else {
        "attention"
    }
}

/// `getProviderSummary`'s headline and detail for a provider that is not ready.
fn summary(p: Option<&Value>) -> String {
    let Some(p) = p else { return "Checking provider status".into() };
    let message = p["message"].as_str().filter(|m| !m.is_empty());
    let (headline, fallback) = if p["installed"] == false {
        ("Not found", Some("CLI not detected on PATH."))
    } else if p["auth"]["status"] == "unauthenticated" {
        ("Not authenticated", None)
    } else if p["status"] == "error" {
        ("Unavailable", Some("The provider failed its startup checks."))
    } else {
        ("Needs attention", Some("The provider is installed, but the server could not fully verify it."))
    };
    match message.or(fallback) {
        Some(detail) => format!("{headline} · {detail}"),
        None => headline.into(),
    }
}

/// `selectOnboardingProvidersByDriver`: the most ready instance of each driver.
fn provider<'a>(s: &'a App, driver: &str) -> Option<&'a Value> {
    let rank = |p: &Value| ["checking", "disabled", "install", "attention", "signIn", "ready"].iter().position(|k| *k == provider_state(Some(p))).unwrap_or(0);
    crate::chat::composer::providers(s).iter().filter(|p| data::s(p, "driver") == driver).max_by_key(|p| rank(p))
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    if !s.ui.wizard.decided && s.state().shell_ready {
        s.ui.wizard.decided = true;
        if !completed(s) {
            if fresh(s.state()) {
                s.ui.wizard.open = true;
                changed = true;
            } else {
                complete(s);
            }
        }
    }
    if !s.ui.wizard.open {
        return changed;
    }
    if let Some(id) = s.ui.wizard.scan
        && let Some(reply) = s.src.net().and_then(|n| n.take_reply(id))
    {
        let ui = &mut s.ui.wizard;
        ui.scan = None;
        match reply {
            Ok(v) => {
                let candidates = v["candidates"].as_array().cloned().unwrap_or_default();
                ui.selected = candidates.iter().filter(|c| c["alreadyImported"] != true).map(|c| data::s(c, "path").to_owned()).collect();
                ui.truncated = v["truncated"] == true;
                ui.candidates = Some(candidates);
            }
            Err(e) => ui.scan_error = Some(crate::shell::palette::error_message(&e, "Could not look for projects.")),
        }
        changed = true;
    }
    changed | poll_import(s)
}

fn poll_import(s: &mut App) -> bool {
    let Some(net) = s.src.net() else { return false };
    let Some(import) = s.ui.wizard.import.as_mut() else { return false };
    let mut changed = false;
    let history = |net: &mut s5_net::Net, project: &str, path: &str| net.request("agentSessions.import", json!({"projectId": project, "expectedWorkspaceRoot": path}));
    loop {
        match import.stage.take() {
            Some(Stage::Create(id, project, path)) => match net.take_reply(id) {
                None => {
                    import.stage = Some(Stage::Create(id, project, path));
                    break;
                }
                Some(Ok(_)) => import.stage = Some(Stage::History(history(net, &project, &path), project)),
                Some(Err(_)) => import.failed += 1,
            },
            Some(Stage::History(id, project)) => match net.take_reply(id) {
                None => {
                    import.stage = Some(Stage::History(id, project));
                    break;
                }
                Some(reply) => {
                    changed = true;
                    match reply {
                        Ok(v) => {
                            import.imported += v["importedCount"].as_u64().unwrap_or(0);
                            import.skipped += v["skippedCount"].as_u64().unwrap_or(0);
                        }
                        Err(_) => import.failed += 1,
                    }
                    import.first_project.get_or_insert(project);
                }
            },
            None => {
                let Some((candidate, project)) = import.queue.pop() else { break };
                let path = data::s(&candidate, "path").to_owned();
                import.stage = Some(match project {
                    Some(project) => Stage::History(history(net, &project, &path), project),
                    None => {
                        let project = s5_net::uuid();
                        let command = json!({
                            "type": "project.create",
                            "commandId": format!("onboarding:project:create:{project}"),
                            "projectId": project,
                            "title": data::s(&candidate, "title"),
                            "workspaceRoot": path,
                            "createWorkspaceRootIfMissing": false,
                            "defaultModelSelection": null,
                        });
                        Stage::Create(net.request("projects.mutate", command), project, path)
                    }
                });
            }
        }
    }
    if import.stage.is_none() && import.queue.is_empty() {
        let Import { first_project, imported, skipped, failed, .. } = s.ui.wizard.import.take().expect("checked above");
        let plural = |n: u64| if n == 1 { "thread" } else { "threads" };
        let warning = if skipped > 0 || failed > 0 {
            Some(if imported > 0 && skipped > 0 {
                format!("Imported {imported} {}. {skipped} {} could not be imported.", plural(imported), plural(skipped))
            } else if imported > 0 {
                format!("Imported {imported} {}. Some thread history could not be imported.", plural(imported))
            } else {
                "Some projects or thread history could not be imported.".into()
            })
        } else {
            None
        };
        finish(s, first_project);
        match warning {
            Some(warning) => crate::shell::palette::toast(s, "Some history was not imported", Some(&warning)),
            None if imported > 0 => crate::shell::palette::toast(s, &format!("Imported {imported} {}", plural(imported)), None),
            None => {}
        }
        return true;
    }
    changed
}

fn finish(s: &mut App, project: Option<String>) {
    complete(s);
    s.ui.wizard.open = false;
    s.ui.wizard.import = None;
    if project.is_some() {
        s.navigate(Route::Home { project_id: project });
    }
}

fn close(s: &mut App) {
    if s.ui.wizard.import.is_none() {
        finish(s, None);
    }
}

fn go(s: &mut App, step: Step) {
    s.ui.wizard.step = step;
    if step == Step::Projects && s.ui.wizard.candidates.is_none() && s.ui.wizard.scan.is_none() {
        s.ui.wizard.scan_error = None;
        s.ui.wizard.scan = s.src.net().map(|n| n.request("agentSessions.scan", json!({})));
    }
}

fn start_import(s: &mut App) {
    let ui = &s.ui.wizard;
    let selected: Vec<Value> = ui.candidates.iter().flatten().filter(|c| ui.selected.contains(data::s(c, "path"))).cloned().collect();
    if selected.is_empty() {
        return finish(s, None);
    }
    let queue = selected
        .into_iter()
        .rev()
        .map(|c| {
            let existing = c["projectId"].as_str().map(str::to_owned).or_else(|| {
                s.state().projects.iter().find(|p| data::s(p, "workspaceRoot") == data::s(&c, "path") && p["archivedAt"].is_null()).map(|p| data::s(p, "id").to_owned())
            });
            (c, existing)
        })
        .collect();
    s.ui.wizard.import = Some(Import { queue, stage: None, first_project: None, imported: 0, skipped: 0, failed: 0 });
}

fn heading(title: &str, description: Option<&str>, t: &T) -> El {
    let mut out = col().w_full().gap(10.).child(text(title.to_owned()).font(theme::weight(theme::TEXT_2XL, 600.)).color(t.c.foreground).wrap());
    if let Some(description) = description {
        out = out.child(text(description.to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap());
    }
    out
}

fn card(t: &T) -> El {
    row().w_full().gap(12.).px(12.).py(12.).center_y().radius(8.).border(1., t.c.border).fill(t.c.background)
}

fn continue_button(t: &T, label: &str, disabled: bool, f: impl Fn(&mut App) + 'static) -> El {
    kit::Button::new(label).icon(&icons::ARROW_RIGHT).disabled(disabled).on_press(f).build(t)
}

fn connect(s: &App, t: &T) -> El {
    let mut body = col().w_full().gap(8.).child(heading("Connect your computers", Some("Choose one or more computers. We’ll set up agents and projects on each."), t)).child(rune::el().h(12.));
    for key in crate::env::keys(s) {
        let active = s.ui.env.active == key;
        let mut line = card(t)
            .child(icon(&icons::MONITOR, 16.).color(t.c.muted_foreground))
            .child(text(crate::env::label(s, &key)).font(theme::SM_MEDIUM).color(t.c.foreground).grow(1.).shrink().min_w(0.).truncate())
            .child(text(crate::env::status(s, &key)).font(theme::TEXT_XS).color(t.c.muted_foreground));
        if active {
            line = line.child(text("Setting up").font(theme::XS_MEDIUM).color(t.c.success_foreground));
        } else {
            let target = key.clone();
            line = line.child(kit::Button::new("Set up").variant("outline").size("xs").on_press(move |s: &mut App| crate::env::switch(s, &target)).build(t));
        }
        body = body.child(line);
    }
    let open = s.ui.wizard.pairing_open;
    let mut pairing = col().w_full().radius(8.).border(1., t.c.border).fill(t.c.background).child(
        row()
            .key("wizard-add-computer")
            .w_full()
            .h(56.)
            .px(12.)
            .gap(8.)
            .center_y()
            .role("button")
            .label("Add a computer")
            .cursor(rune::Cursor::Pointer)
            .child(icon(&icons::LINK, 16.).color(t.c.muted_foreground))
            .child(text("Add a computer").font(theme::SM_MEDIUM).color(t.c.foreground).grow(1.))
            .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 16.).color(t.c.muted_foreground))
            .on_press(|s: &mut App| s.ui.wizard.pairing_open = !s.ui.wizard.pairing_open),
    );
    if open {
        pairing = pairing.child(crate::env::pairing_form(s, t));
    }
    let ready = crate::env::status(s, &s.ui.env.active) == "Connected";
    body.child(rune::el().h(8.)).child(pairing).child(rune::el().h(16.)).child(
        row()
            .w_full()
            .gap(24.)
            .center_y()
            .child(
                text("T3 Code collects anonymous usage data to help us improve it. To read more about how your data is used and how to opt out, see our privacy policy.")
                    .font(theme::TEXT_XS)
                    .color(t.c.muted_foreground)
                    .wrap()
                    .grow(1.)
                    .shrink()
                    .min_w(0.)
                    .role("link")
                    .cursor(rune::Cursor::Pointer)
                    .on_press_tx(|_: &mut App, tx: &mut rune::Tx| {
                        let _ = tx.open_url(PRIVACY_POLICY_URL);
                    }),
            )
            .child(continue_button(t, "Continue", !ready, |s: &mut App| go(s, Step::Agents))),
    )
}

fn agents(s: &App, t: &T) -> El {
    let mut list = col().w_full().gap(8.).child(text(crate::env::label(s, &s.ui.env.active)).font(theme::SM_MEDIUM).color(t.c.foreground));
    for (driver, name) in AGENTS {
        let p = provider(s, driver);
        let state = provider_state(p);
        let display = p.map(|p| data::s(p, "displayName")).filter(|n| !n.is_empty()).unwrap_or(name).to_owned();
        let status: El = match state {
            "ready" => row().gap(6.).center_y().child(icon(&icons::CHECK, 14.).color(t.c.success_foreground)).child(text("Ready").font(theme::XS_MEDIUM).color(t.c.success_foreground)),
            "checking" => text("Checking...").font(theme::TEXT_XS).color(t.c.muted_foreground),
            "disabled" => text("Disabled").font(theme::TEXT_XS).color(t.c.muted_foreground),
            "attention" => text("Needs attention").font(theme::TEXT_XS).color(t.c.muted_foreground),
            // The web runs the install or login command in an embedded terminal; Rune hands
            // off to Settings > Providers, which has the same checks.
            _ => kit::Button::new(if state == "signIn" { "Sign in" } else { "Install" })
                .icon(&icons::TERMINAL)
                .variant("ghost")
                .size("xs")
                .on_press(|s: &mut App| s.navigate(Route::Settings("providers".into())))
                .build(t),
        };
        list = list.child(
            card(t)
                .px(16.)
                .py(16.)
                .child(crate::chat::composer::provider_icon(t, driver, 20.))
                .child(
                    col()
                        .grow(1.)
                        .shrink()
                        .min_w(0.)
                        .gap(2.)
                        .child(text(display).font(theme::SM_MEDIUM).color(t.c.foreground))
                        .child(text(if state == "ready" { "Ready to code.".to_owned() } else { summary(p) }).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()),
                )
                .child(status),
        );
    }
    col()
        .w_full()
        .gap(20.)
        .child(heading("Connect your agents", Some("Choose an agent to start coding. You can add more later."), t))
        .child(list)
        .child(row().w_full().child(rune::el().grow(1.)).child(continue_button(t, "Continue", false, |s: &mut App| go(s, Step::Projects))))
}

fn projects(s: &App, t: &T) -> El {
    let ui = &s.ui.wizard;
    let importing = ui.import.is_some();
    let skip = kit::Button::new("Do not import projects").variant("ghost-muted").disabled(importing).on_press(|s: &mut App| finish(s, None)).build(t);
    let Some(candidates) = &ui.candidates else {
        let message = ui.scan_error.clone().unwrap_or_else(|| "Looking for projects from Claude Code and Codex…".into());
        return col()
            .w_full()
            .min_h(160.)
            .child(heading("Your projects", None, t))
            .child(
                col()
                    .grow(1.)
                    .center()
                    .gap(12.)
                    .py(24.)
                    .children(ui.scan_error.is_none().then(|| kit::Spinner::new().size("lg").tone("muted").build(&kit::theme(t))))
                    .child(text(message).font(theme::TEXT_SM).color(t.c.muted_foreground)),
            )
            .child(row().w_full().child(rune::el().grow(1.)).child(skip));
    };
    let mut body = col().w_full().gap(12.).child(heading("Import your projects", Some("Import projects and conversations from Claude Code and Codex on your selected computers."), t));
    if candidates.is_empty() {
        body = body.child(text("No projects found.").font(theme::TEXT_SM).color(t.c.muted_foreground));
    } else {
        let total = candidates.len();
        let chosen = candidates.iter().filter(|c| ui.selected.contains(data::s(c, "path"))).count();
        body = body.child(
            row()
                .w_full()
                .gap(4.)
                .center_y()
                .child(text(format!("{chosen} of {total} selected")).font(theme::TEXT_XS).color(t.c.muted_foreground).grow(1.))
                .child(
                    kit::Button::new("Select all")
                        .variant("ghost")
                        .size("xs")
                        .disabled(importing || chosen == total)
                        .on_press(|s: &mut App| {
                            let paths: Vec<String> = s.ui.wizard.candidates.iter().flatten().map(|c| data::s(c, "path").to_owned()).collect();
                            s.ui.wizard.selected.extend(paths);
                        })
                        .build(t),
                )
                .child(kit::Button::new("Select none").variant("ghost").size("xs").disabled(importing || chosen == 0).on_press(|s: &mut App| s.ui.wizard.selected.clear()).build(t)),
        );
        let mut list = col().w_full().gap(2.);
        for c in candidates {
            let path = data::s(c, "path").to_owned();
            let checked = ui.selected.contains(&path);
            let meta = format!("{} · {}", c["sources"].as_array().into_iter().flatten().filter_map(|v| v.as_str()).map(|v| if v == "claudeAgent" { "Claude Code" } else if v == "codex" { "Codex" } else { v }).collect::<Vec<_>>().join(", "), c["threadCount"].as_u64().unwrap_or(0));
            let toggle = path.clone();
            list = list.child(
                row()
                    .w_full()
                    .gap(10.)
                    .px(8.)
                    .py(8.)
                    .radius(6.)
                    .center_y()
                    .hover(t.c.accent)
                    .child(kit::Checkbox::new(checked).hide_label().label(data::s(c, "title")).disabled(importing).build(&kit::theme(t)))
                    .child(
                        col()
                            .grow(1.)
                            .shrink()
                            .min_w(0.)
                            .child(text(data::s(c, "title").to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
                            .child(text(path.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate()),
                    )
                    .child(text(meta).font(theme::TEXT_XS).color(t.c.muted_foreground))
                    .on_press(move |s: &mut App| {
                        if s.ui.wizard.import.is_some() {
                            return;
                        }
                        if !s.ui.wizard.selected.remove(&toggle) {
                            s.ui.wizard.selected.insert(toggle.clone());
                        }
                    }),
            );
        }
        body = body.child(kit::scroll_area("wizard-import-list", list.scroll(rune::ScrollSpec { stick_bottom: false }), t).max_h(320.));
        if ui.truncated {
            body = body.child(text("Scan limit reached. Some projects or conversations may be missing.").font(theme::TEXT_XS).color(t.c.muted_foreground));
        }
    }
    let label = if importing { "Importing…" } else { "Import" };
    body.child(rune::el().h(8.)).child(row().w_full().gap(8.).child(rune::el().grow(1.)).child(skip).child(continue_button(t, label, importing || ui.selected.is_empty(), start_import)))
}

pub fn overlays(s: &App, t: &T, layer: &mut Vec<El>) {
    let ui = &s.ui.wizard;
    if !ui.open {
        return;
    }
    let current = match ui.step {
        Step::Connect => 0,
        Step::Agents => 1,
        Step::Projects => 2,
    };
    let importing = ui.import.is_some();
    let mut steps = row().gap(6.).center_y();
    for (i, name) in STAGES.iter().enumerate() {
        if i > 0 {
            steps = steps.child(rune::el().w(16.).h(1.).fill(t.c.border));
        }
        let tone = if i == current { t.c.foreground } else { t.c.muted_foreground };
        let mut chip = row()
            .gap(6.)
            .center_y()
            .child(row().size(18., 18.).center().radius(9.).fill(if i <= current { t.c.primary } else { t.c.muted }).child(text((i + 1).to_string()).font(theme::XS_MEDIUM).color(if i <= current { t.c.primary_foreground } else { t.c.muted_foreground })))
            .child(text(name.to_string()).font(theme::XS_MEDIUM).color(tone));
        if i < current && !importing {
            chip = chip.role("button").label(*name).cursor(rune::Cursor::Pointer).on_press(move |s: &mut App| go(s, if i == 0 { Step::Connect } else { Step::Agents }));
        }
        steps = steps.child(chip);
    }
    let content = match ui.step {
        Step::Connect => connect(s, t),
        Step::Agents => agents(s, t),
        Step::Projects => projects(s, t),
    };
    kit::Dialog::new("welcome-wizard", "Set up T3 Code")
        .width(640.)
        .alert()
        .on_close(close)
        .child(steps)
        .child(rune::el().w_full().h(1.).fill(t.c.border.alpha_mul(0.8)))
        .child(content)
        .open(true)
        .build(t, layer);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn onboarding_provider_states_match_the_web() {
        assert_eq!(provider_state(None), "checking");
        let p = |v: Value| provider_state(Some(&v));
        assert_eq!(p(json!({"enabled": false, "status": "ready"})), "disabled");
        assert_eq!(p(json!({"enabled": true, "installed": false, "status": "warning", "auth": {"status": "unknown"}})), "checking");
        assert_eq!(p(json!({"enabled": true, "installed": false, "status": "error", "auth": {"status": "unknown"}})), "install");
        assert_eq!(p(json!({"enabled": true, "installed": true, "status": "ready", "auth": {"status": "unauthenticated"}})), "signIn");
        assert_eq!(p(json!({"enabled": true, "installed": true, "status": "ready", "auth": {"status": "authenticated"}})), "ready");
        assert_eq!(p(json!({"enabled": true, "installed": true, "status": "warning", "auth": {"status": "authenticated"}})), "attention");
        assert_eq!(summary(Some(&json!({"installed": false}))), "Not found · CLI not detected on PATH.");
    }

    #[test]
    fn the_wizard_opens_once_on_a_fresh_workspace_and_finishing_records_it() {
        let dir = std::env::temp_dir().join(format!("s5-wizard-{}", std::process::id()));
        // SAFETY: tests that read the settings path set it to their own file first.
        unsafe { std::env::set_var("S5_CLIENT_SETTINGS", dir.join("settings.json")) };
        let mut s = App::new();
        let mut state = data::empty_state();
        state.shell_ready = true;
        s.src = crate::data::Source::Fixture(state);
        s.client_settings.values.insert("onboardingCompletedAt".into(), Value::Null);
        assert!(poll(&mut s) && s.ui.wizard.open);
        close(&mut s);
        assert!(!s.ui.wizard.open && completed(&s));
        assert!(!poll(&mut s) && !s.ui.wizard.open);

        let mut s = App::new();
        let mut state = data::fixture_state();
        state.shell_ready = true;
        s.src = crate::data::Source::Fixture(state);
        s.client_settings.values.insert("onboardingCompletedAt".into(), Value::Null);
        poll(&mut s);
        assert!(!s.ui.wizard.open && completed(&s), "a used workspace skips the wizard and records it");
    }
}
