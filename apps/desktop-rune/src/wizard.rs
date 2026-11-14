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
/// `RECENT_PROJECT_WINDOW_MS` and `DEFAULT_SELECTION_MIN_THREADS` (onboarding/projectImport.logic.ts).
const RECENT_WINDOW_MS: i64 = 30 * 24 * 60 * 60 * 1000;
const DEFAULT_SELECTION_MIN_THREADS: u64 = 3;
/// `AgentSessionSource.literals`, each with its `IMPORT_SOURCE_NAMES` label.
const SOURCES: [(&str, &str); 3] = [("claudeAgent", "Claude Code"), ("codex", "Codex"), ("pi", "Pi")];
const SCAN_LIMIT_MESSAGE: &str = "Scan limit reached. Some projects or conversations may be missing.";
/// The "Other folders" section's key in `Ui::toggled`.
const OTHER: &str = "other";

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
    /// The environment scanned, so the import lands there whatever is focused by then.
    scan_env: String,
    candidates: Option<Vec<Value>>,
    truncated: bool,
    scan_error: Option<String>,
    /// Candidate paths to import.
    selected: HashSet<String>,
    /// Import sections opened or closed away from their default (groups open, other folders closed).
    toggled: HashSet<String>,
    import: Option<Import>,
}

/// One candidate at a time: create its project when it has none, then import its history.
struct Import {
    env: String,
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
                // `expandPairingInitially`: with no local backend, pairing is the only way on.
                s.ui.wizard.pairing_open = s.ui.desktop.local_off;
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
                let now = data::now_ms();
                ui.selected = candidates.iter().filter(|c| recent(c, now)).map(|c| data::s(c, "path").to_owned()).collect();
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
    let Some(mut import) = s.ui.wizard.import.take() else { return false };
    let mut changed = false;
    let reply = |s: &mut App, id: u64| s.src.net().and_then(|n| n.take_reply(id));
    let send = |s: &mut App, env: &str, method: &str, params: Value| crate::env::net_of(s, env).map(|n| n.request(method, params));
    let history = |s: &mut App, env: &str, project: &str, path: &str| send(s, env, "agentSessions.import", json!({"projectId": project, "expectedWorkspaceRoot": path}));
    loop {
        match import.stage.take() {
            Some(Stage::Create(id, project, path)) => match reply(s, id) {
                None => {
                    import.stage = Some(Stage::Create(id, project, path));
                    break;
                }
                Some(Ok(_)) => match history(s, &import.env, &project, &path) {
                    Some(id) => import.stage = Some(Stage::History(id, project)),
                    None => import.failed += 1,
                },
                Some(Err(_)) => import.failed += 1,
            },
            Some(Stage::History(id, project)) => match reply(s, id) {
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
                let stage = match project {
                    Some(project) => history(s, &import.env, &project, &path).map(|id| Stage::History(id, project)),
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
                        crate::env::adopt(s, &project, &import.env);
                        send(s, &import.env, "projects.mutate", command).map(|id| Stage::Create(id, project, path))
                    }
                };
                match stage {
                    Some(stage) => import.stage = Some(stage),
                    None => import.failed += 1,
                }
            }
        }
    }
    if import.stage.is_none() && import.queue.is_empty() {
        let Import { env, first_project, imported, skipped, failed, .. } = import;
        if let Some(project) = &first_project {
            crate::env::adopt(s, project, &env);
        }
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
    s.ui.wizard.import = Some(import);
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
        scan(s);
    }
}

fn scan(s: &mut App) {
    s.ui.wizard.scan_error = None;
    s.ui.wizard.scan = s.src.net().map(|n| n.request("agentSessions.scan", json!({})));
    s.ui.wizard.scan_env = s.ui.env.active.clone();
}

/// `partitionOnboardingProjects`' `recent`, which starts selected: a git repository with a few
/// threads, active in the last month. A server predating git scans sends no `git` at all.
fn recent(c: &Value, now: i64) -> bool {
    !c.get("git").is_some_and(Value::is_null)
        && c["threadCount"].as_u64().unwrap_or(0) >= DEFAULT_SELECTION_MIN_THREADS
        && data::ts(c, "lastActiveAt").is_some_and(|at| at >= now - RECENT_WINDOW_MS && at <= now)
}

#[derive(Debug, PartialEq)]
struct Group {
    key: String,
    label: String,
    /// Indexes into the candidates.
    members: Vec<usize>,
    threads: u64,
    last_active: Option<i64>,
}

/// `groupOnboardingProjects`: candidates by repository, most recently active first, and the
/// folders outside git.
fn groups(candidates: &[Value]) -> (Vec<Group>, Vec<usize>) {
    let mut groups: Vec<Group> = vec![];
    let mut other = vec![];
    for (i, c) in candidates.iter().enumerate() {
        let git = c.get("git");
        if git.is_some_and(Value::is_null) {
            other.push(i);
            continue;
        }
        let key = match git.and_then(|g| g["remoteKey"].as_str()) {
            Some(remote) => format!("remote:{remote}"),
            None => format!("path:{}", data::s(c, "path")),
        };
        let (threads, active) = (c["threadCount"].as_u64().unwrap_or(0), data::ts(c, "lastActiveAt"));
        match groups.iter_mut().find(|g| g.key == key) {
            Some(g) => {
                g.members.push(i);
                g.threads += threads;
                g.last_active = g.last_active.max(active);
            }
            None => groups.push(Group {
                key,
                label: git.and_then(|g| g["repository"].as_str()).unwrap_or(data::s(c, "title")).to_owned(),
                members: vec![i],
                threads,
                last_active: active,
            }),
        }
    }
    groups.sort_by(|a, b| match (a.last_active, b.last_active) {
        (x, y) if x == y => a.label.cmp(&b.label),
        (None, _) => std::cmp::Ordering::Greater,
        (_, None) => std::cmp::Ordering::Less,
        (Some(x), Some(y)) => y.cmp(&x),
    });
    (groups, other)
}

/// `splitOnboardingProjectPath`: the folder name and the path before it.
fn split_path(path: &str) -> (&str, &str) {
    let trimmed = path.trim_end_matches(['/', '\\']);
    if trimmed.is_empty() {
        return ("", path);
    }
    match trimmed.rfind(['/', '\\']) {
        Some(i) => (&trimmed[..=i], &trimmed[i + 1..]),
        None => ("", trimmed),
    }
}

fn set_selected(s: &mut App, paths: &[String], on: bool) {
    if s.ui.wizard.import.is_some() {
        return;
    }
    for path in paths {
        if on {
            s.ui.wizard.selected.insert(path.clone());
        } else {
            s.ui.wizard.selected.remove(path);
        }
    }
}

fn toggle_section(s: &mut App, key: &str) {
    if !s.ui.wizard.toggled.remove(key) {
        s.ui.wizard.toggled.insert(key.to_owned());
    }
}

/// `ImportRowMeta`: which agents left history, how many threads, and how long ago.
fn row_meta(sources: Option<&Value>, threads: u64, last_active: Option<i64>, t: &T) -> El {
    let muted = t.c.muted_foreground;
    let mut out = row().gap(4.).center_y();
    for (source, name) in SOURCES {
        let has = sources.and_then(Value::as_array).is_some_and(|list| list.iter().any(|v| v == source));
        out = out.child(row().size(16., 16.).center().children(has.then(|| crate::chat::composer::provider_icon(t, source, 12.).label(name))));
    }
    let age = last_active.map(|at| crate::shell::sidebar::logic::relative_label(at, data::now_ms())).unwrap_or_default();
    out.child(row().w(40.).justify_end().child(text(threads.to_string()).font(theme::TEXT_XS).color(muted)))
        .child(row().w(36.).justify_end().child(text(age).font(theme::TEXT_XS).color(muted)))
}

/// `ImportCandidateRow`: a top-level row names its repository; a nested one shows only its path.
fn candidate_row(c: &Value, label: Option<&str>, nested: bool, ui: &Ui, t: &T) -> El {
    let path = data::s(c, "path").to_owned();
    let (parent, name) = split_path(&path);
    let (font, ink) = if label.is_some() { (theme::mono(theme::TEXT_2XS), t.c.muted_foreground) } else { (theme::mono(theme::TEXT_XS), t.c.foreground) };
    let mut info = row().grow(1.).shrink().min_w(0.).gap(8.).center_y().help(path.clone());
    if let Some(label) = label {
        info = info.child(text(label.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate().shrink().min_w(0.));
    }
    info = info.child(row().shrink().min_w(0.).child(text(parent.to_owned()).font(font).color(ink).truncate().shrink().min_w(0.)).child(text(name.to_owned()).font(font).color(ink).truncate()));
    let checked = ui.selected.contains(&path);
    let toggle = path.clone();
    row()
        .w_full()
        .gap(10.)
        .pr(8.)
        .pl(if nested { 32. } else { 8. })
        .py(6.)
        .radius(6.)
        .center_y()
        .hover(t.c.muted.alpha_mul(0.4))
        .child(kit::Checkbox::new(checked).hide_label().label(label.unwrap_or(&path)).disabled(ui.import.is_some()).build(&kit::theme(t)))
        .child(info)
        .child(row_meta((!nested).then(|| &c["sources"]), c["threadCount"].as_u64().unwrap_or(0), data::ts(c, "lastActiveAt"), t))
        .on_press(move |s: &mut App| set_selected(s, std::slice::from_ref(&toggle), !checked))
}

/// A collapsible section's header: its checkbox selects every member, the rest opens or closes it.
fn section_header(key: &str, open: bool, paths: Vec<String>, ui: &Ui, title: El, meta: El, t: &T) -> El {
    let chosen = paths.iter().filter(|p| ui.selected.contains(*p)).count();
    let all = chosen == paths.len();
    let key = key.to_owned();
    row()
        .w_full()
        .gap(10.)
        .px(8.)
        .py(6.)
        .radius(6.)
        .center_y()
        .hover(t.c.muted.alpha_mul(0.4))
        .child(
            kit::Checkbox::new(all)
                .indeterminate(chosen > 0 && !all)
                .hide_label()
                .label("Select section")
                .disabled(ui.import.is_some())
                .on_toggle(move |s: &mut App| set_selected(s, &paths, !all))
                .build(&kit::theme(t)),
        )
        .child(
            row()
                .grow(1.)
                .shrink()
                .min_w(0.)
                .gap(6.)
                .center_y()
                .role("button")
                .cursor(rune::Cursor::Pointer)
                .child(icon(if open { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 14.).color(t.c.muted_foreground))
                .child(title.shrink().min_w(0.))
                .child(rune::el().grow(1.))
                .child(meta)
                .on_press(move |s: &mut App| toggle_section(s, &key)),
        )
}

/// `ImportCandidateList`.
fn import_list(candidates: &[Value], ui: &Ui, t: &T) -> El {
    let (repositories, other) = groups(candidates);
    let paths = |members: &[usize]| members.iter().map(|&i| data::s(&candidates[i], "path").to_owned()).collect::<Vec<_>>();
    let mut list = col().w_full().gap(2.);
    for group in &repositories {
        if let [only] = group.members[..] {
            list = list.child(candidate_row(&candidates[only], Some(&group.label), false, ui, t));
            continue;
        }
        let open = !ui.toggled.contains(&group.key);
        let mut sources: Vec<Value> = vec![];
        for &i in &group.members {
            for source in candidates[i]["sources"].as_array().into_iter().flatten() {
                if !sources.contains(source) {
                    sources.push(source.clone());
                }
            }
        }
        let title = text(group.label.clone()).font(theme::SM_MEDIUM).color(t.c.foreground).truncate();
        list = list.child(section_header(&group.key, open, paths(&group.members), ui, title, row_meta(Some(&Value::Array(sources)), group.threads, group.last_active, t), t));
        if open {
            for &i in &group.members {
                list = list.child(candidate_row(&candidates[i], None, true, ui, t));
            }
        }
    }
    if !other.is_empty() {
        let open = ui.toggled.contains(OTHER);
        let count = format!("{} {}", other.len(), if other.len() == 1 { "folder" } else { "folders" });
        let title = text("Other folders").font(theme::TEXT_SM).color(t.c.muted_foreground).truncate();
        list = list.child(section_header(OTHER, open, paths(&other), ui, title, text(count).font(theme::TEXT_XS).color(t.c.muted_foreground), t));
        if open {
            for &i in &other {
                list = list.child(candidate_row(&candidates[i], None, true, ui, t));
            }
        }
    }
    list
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
                s.state()
                    .projects
                    .iter()
                    .find(|p| data::s(p, "workspaceRoot") == data::s(&c, "path") && p["archivedAt"].is_null() && crate::env::owner(s, data::s(p, "id")) == ui.scan_env)
                    .map(|p| data::s(p, "id").to_owned())
            });
            (c, existing)
        })
        .collect();
    s.ui.wizard.import = Some(Import { env: s.ui.wizard.scan_env.clone(), queue, stage: None, first_project: None, imported: 0, skipped: 0, failed: 0 });
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
            .child(
                col()
                    .grow(1.)
                    .shrink()
                    .min_w(0.)
                    .gap(2.)
                    .child(text(crate::env::label(s, &key)).font(theme::SM_MEDIUM).color(t.c.foreground).truncate())
                    .children(crate::env::base_url(s, &key).map(|url| text(url).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate())),
            )
            .child(text(crate::env::short_status(s, &key)).font(theme::TEXT_XS).color(t.c.muted_foreground).help(crate::env::status(s, &key)));
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
            // off to Settings > Providers, which has the same checks (see `overlays`).
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
    if ui.candidates.is_none() && ui.scan_error.is_none() {
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
                    .child(kit::Spinner::new().size("lg").tone("muted").build(&kit::theme(t)))
                    .child(text("Looking for projects from Claude Code and Codex…").font(theme::TEXT_SM).color(t.c.muted_foreground)),
            )
            .child(row().w_full().child(rune::el().grow(1.)).child(skip));
    }
    let candidates = ui.candidates.as_deref().unwrap_or_default();
    let mut body = col().w_full().gap(12.).child(heading("Import your projects", Some("Import projects and conversations from Claude Code and Codex on your selected computers."), t));
    let chosen = candidates.iter().filter(|c| ui.selected.contains(data::s(c, "path"))).count();
    if !candidates.is_empty() {
        let total = candidates.len();
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
                            set_selected(s, &paths, true);
                        })
                        .build(t),
                )
                .child(kit::Button::new("Select none").variant("ghost").size("xs").disabled(importing || chosen == 0).on_press(|s: &mut App| s.ui.wizard.selected.clear()).build(t)),
        );
    }
    let mut list = col().w_full().gap(2.);
    if let Some(error) = &ui.scan_error {
        list = list.child(
            row()
                .w_full()
                .gap(12.)
                .center_y()
                .child(text(format!("Could not check projects. {error}")).font(theme::TEXT_SM).color(t.c.muted_foreground).wrap().grow(1.).shrink().min_w(0.))
                .child(kit::Button::new("Retry").variant("ghost").size("sm").on_press(scan).build(t)),
        );
    } else if candidates.is_empty() {
        list = list.child(text("No existing Claude Code or Codex projects found.").font(theme::TEXT_SM).color(t.c.muted_foreground).py(8.));
    }
    if ui.truncated {
        list = list.child(text(SCAN_LIMIT_MESSAGE).font(theme::TEXT_XS).color(t.c.muted_foreground));
    }
    list = list.child(import_list(candidates, ui, t));
    body = body.child(kit::scroll_area("wizard-import-list", list.scroll(rune::ScrollSpec { stick_bottom: false }), t).max_h(320.));
    let label = if importing { "Importing…".into() } else { format!("Import {chosen} {}", if chosen == 1 { "project" } else { "projects" }) };
    body.child(rune::el().h(8.)).child(row().w_full().gap(8.).child(rune::el().grow(1.)).child(skip).child(continue_button(t, &label, importing || chosen == 0, start_import)))
}

pub fn overlays(s: &App, t: &T, layer: &mut Vec<El>) {
    let ui = &s.ui.wizard;
    // Sign in / Install hand off to Settings > Providers; the wizard steps aside there and
    // comes back on the Agents step when the person leaves.
    if !ui.open || matches!(&s.route, Route::Settings(p) if p == "providers") {
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
        .button_only()
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
    fn import_candidates_group_by_repository_and_recent_ones_start_selected() {
        let now = data::iso_ms("2026-11-09T12:00:00.000Z").unwrap();
        let c = |path: &str, git: Option<Value>, threads: u64, at: Option<&str>| {
            let mut v = json!({"path": path, "title": path.rsplit('/').next(), "sources": ["codex"], "threadCount": threads, "lastActiveAt": at, "alreadyImported": false});
            if let Some(git) = git {
                v["git"] = git;
            }
            v
        };
        let repo = |name: &str| json!({"remoteKey": format!("github.com/{name}"), "repository": name});
        let candidates = [
            c("/w/app", Some(repo("me/app")), 5, Some("2026-11-01T00:00:00.000Z")),
            c("/w/app-wt", Some(repo("me/app")), 1, Some("2026-11-08T00:00:00.000Z")),
            c("/w/old", Some(repo("me/old")), 9, Some("2026-08-01T00:00:00.000Z")),
            c("/w/legacy", None, 3, Some("2026-11-05T00:00:00.000Z")),
            c("/tmp/notes", Some(Value::Null), 7, Some("2026-11-08T00:00:00.000Z")),
        ];
        let picked: Vec<&str> = candidates.iter().filter(|c| recent(c, now)).map(|c| data::s(c, "path")).collect();
        assert_eq!(picked, ["/w/app", "/w/legacy"], "few threads, a stale month, or no git leave a project unselected");

        let (repositories, other) = groups(&candidates);
        let summary: Vec<(&str, &[usize], u64)> = repositories.iter().map(|g| (g.label.as_str(), &g.members[..], g.threads)).collect();
        assert_eq!(summary, [("me/app", &[0, 1][..], 6), ("legacy", &[3][..], 3), ("me/old", &[2][..], 9)]);
        assert_eq!(repositories[0].last_active, data::ts(&candidates[1], "lastActiveAt"));
        assert_eq!(other, [4]);
        assert_eq!(split_path("/w/app/"), ("/w/", "app"));
        assert_eq!(split_path("app"), ("", "app"));
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
