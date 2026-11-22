//! The new-thread draft that `/` opens (routes/_chat.index.tsx starts a draft in the most recent
//! project): the draft hero over the composer (DraftHeroHeadline), or a loaded thread's empty
//! prompt. Sending from the composer launches the thread and navigates to it.

use rune::tok::TextStyle;
use rune::{Cursor, Cx, El, Tx, col, row, text};
use serde_json::{Value, json};

use crate::chat::composer;
use crate::chat::details::Lane;
use crate::data;
use crate::theme::{self, T};
use crate::{App, Route};

#[derive(Default)]
pub struct Ui {
    /// The checkout a "New thread on {branch}" draft launches in: `(project id, workspaceStrategy)`.
    pub workspace: Option<(String, Value)>,
    /// When the triage hero's "or run npx t3 triage" last copied.
    triage_copied: Option<i64>,
}

/// "New thread on {branch}": a draft in `project` that launches on the thread's branch and checkout.
pub fn start_on_branch(s: &mut App, project: &str, branch: &str, worktree: Option<&str>) {
    s.ui.chat.home.workspace = Some((project.to_string(), workspace_strategy(branch, worktree)));
    crate::shell::sidebar::go(s, Route::Home { project_id: Some(project.to_string()) });
}

/// `workspaceStrategy` for a draft on `branch`, in its worktree when it has one.
pub fn workspace_strategy(branch: &str, worktree: Option<&str>) -> Value {
    let mut strategy = match worktree.filter(|w| !w.is_empty()) {
        Some(path) => json!({"type": "existing_worktree", "worktreePath": path}),
        None => json!({"type": "root"}),
    };
    // The launch schema rejects an empty branch.
    if !branch.trim().is_empty() {
        strategy["branch"] = json!(branch);
    }
    strategy
}

/// Takes the draft's launch strategy for `project`: its branch preset, else the project root.
pub fn take_workspace(s: &mut App, project: &str) -> Value {
    match s.ui.chat.home.workspace.take() {
        Some((id, strategy)) if id == project => strategy,
        _ => json!({"type": "root"}),
    }
}

/// The project the draft belongs to: the route's, else the one with the latest activity.
pub fn draft_project(s: &App) -> Option<&Value> {
    if let Route::Home { project_id: Some(id) } = s.frame_route()
        && let Some(p) = composer::project_by_id(s, id)
    {
        return Some(p);
    }
    default_project(s)
}

/// The project a bare new-thread draft opens in: the most recently active one.
pub fn default_project(s: &App) -> Option<&Value> {
    let st = s.state();
    // sortScopedProjectsForSidebar by "updated_at": a project with threads ranks by its latest
    // user message, else by its own timestamps; ties go by title.
    let thread_ts = |t: &Value| data::ts(t, "latestUserMessageAt").or_else(|| data::ts(t, "updatedAt")).or_else(|| data::ts(t, "createdAt")).unwrap_or(i64::MIN);
    let activity = |p: &Value| {
        let id = data::s(p, "id");
        let mut threads = st.threads.iter().filter(|t| data::s(t, "projectId") == id && t["archivedAt"].is_null()).peekable();
        if threads.peek().is_some() {
            threads.map(thread_ts).max().unwrap_or(i64::MIN)
        } else {
            data::ts(p, "updatedAt").or_else(|| data::ts(p, "createdAt")).unwrap_or(i64::MIN)
        }
    };
    st.projects
        .iter()
        .filter(|p| p["archivedAt"].is_null())
        .max_by(|a, b| activity(a).cmp(&activity(b)).then_with(|| data::s(b, "title").cmp(data::s(a, "title"))))
}

/// What the draft hero asks (DraftHeroHeadline).
#[derive(Clone, Debug, PartialEq)]
enum Hero {
    /// "What should we build in {project}?"
    Project(String),
    /// "What should we work on?", for a draft without a project.
    Scratch,
    /// "What went wrong?", for a draft without a project that links the triage playbook.
    Triage,
    /// "{Choose a project} to start".
    Choose,
    /// "Add a project to start".
    AddProject,
}

/// `project` is the draft's title and whether it is the scratch project; `triage`, whether the
/// draft links the triage playbook.
fn hero(project: Option<(&str, bool)>, can_choose: bool, triage: bool) -> Hero {
    match project {
        Some((_, true)) if triage => Hero::Triage,
        Some((_, true)) => Hero::Scratch,
        Some((title, false)) => Hero::Project(title.to_string()),
        None if can_choose => Hero::Choose,
        None => Hero::AddProject,
    }
}

/// "or start without a project" shows for a draft that has, or can choose, a project.
fn offers_scratch(hero: &Hero, scratch_available: bool) -> bool {
    scratch_available && matches!(hero, Hero::Project(_) | Hero::Choose)
}

const HEADLINE: TextStyle = theme::TEXT_3XL.tracking(-0.75);

/// The project name that opens the project picker ("New thread in...").
fn project_picker(t: &T, name: &str, font: TextStyle) -> El {
    row()
        .key("home-project-picker")
        .shrink()
        .min_w(0.0)
        .max_w(420.0)
        .hover_group()
        .cursor(Cursor::Pointer)
        .focusable(true)
        .role("combobox")
        .label(name.to_string())
        .help(name.to_string())
        .on_press(crate::shell::palette::open_new_thread_in)
        .child(text(name.to_string()).font(font).color(t.c.foreground).single_line().truncate().shrink().min_w(0.0))
}

fn headline(t: &T, hero: &Hero, width: f32) -> El {
    let line = |label: &str| text(label.to_string()).font(HEADLINE).color(t.c.foreground);
    let (el, label) = match hero {
        Hero::Project(name) => {
            let name_font = theme::weight(HEADLINE, theme::MEDIUM);
            // DraftHeroProjectHeading: one line when it fits, else the name under the question.
            let inline = (name.chars().count() + 25) as f32 * 15.0 <= width;
            let el = if inline {
                row().gap(8.0).justify_center().max_w(width).child(line("What should we build in")).child(row().shrink().min_w(0.0).child(project_picker(t, name, name_font)).child(line("?")))
            } else {
                col().items_center().gap(8.0).max_w(width).child(line("What should we build?")).child(project_picker(t, name, theme::weight(theme::TEXT_BASE, theme::MEDIUM)))
            };
            (el, format!("What should we build in {name}?"))
        }
        Hero::Scratch => (line("What should we work on?"), "What should we work on?".to_string()),
        Hero::Triage => (line("What went wrong?"), "What went wrong?".to_string()),
        Hero::Choose => (row().gap(8.0).justify_center().child(project_picker(t, "Choose a project", HEADLINE)).child(line("to start")), "Choose a project to start".to_string()),
        Hero::AddProject => (line("Add a project to start"), "Add a project to start".to_string()),
    };
    el.key("home-headline").role("heading").label(label)
}

/// The line under the headline: the scratch draft's project picker, or "or start without a
/// project".
fn subline(s: &App, t: &T, hero: &Hero) -> Option<El> {
    if *hero == Hero::Triage {
        return Some(triage_line(s, t));
    }
    let scratch_available = crate::shell::palette::actions::scratch_available(s);
    if *hero == Hero::Scratch && scratch_available {
        return Some(project_picker(t, "No project", theme::TEXT_SM));
    }
    if !offers_scratch(hero, scratch_available) {
        return None;
    }
    let shortcut = crate::shell::palette::shortcut_label(s, "chat.newWithoutProject");
    let mut button = row()
        .key("home-without-project")
        .hover_group()
        .cursor(Cursor::Pointer)
        .focusable(true)
        .role("button")
        .label("or start without a project")
        .on_press(crate::shell::palette::actions::new_without_project)
        .child(text("or start without a project").font(theme::SM_MEDIUM).color(t.c.muted_foreground).group_color(t.c.foreground));
    if !shortcut.is_empty() {
        button = button.help(shortcut);
    }
    Some(button)
}

const TRIAGE_COPIED_MS: i64 = 1500;

/// TriageDraftHero's line: the machine the agent works on and its version, then the command
/// to run in a terminal instead.
fn triage_line(s: &App, t: &T) -> El {
    let key = draft_project(s).map_or_else(|| s.ui.env.active.clone(), |p| crate::env::of(s, p));
    let label = Some(crate::env::label(s, &key)).filter(|l| !l.is_empty()).unwrap_or_else(|| "This machine".to_owned());
    let config = crate::env::source(s, &key).and_then(|src| src.state().config.as_ref());
    let version = config.and_then(|c| c["environment"]["serverVersion"].as_str()).filter(|v| !v.is_empty());
    let machine = row()
        .key("home-triage-machine")
        .shrink()
        .min_w(0.0)
        .center_y()
        .gap(6.0)
        .focusable(true)
        .help("The agent reads T3 Code's logs and source here, and asks before it applies a fix or posts anything.")
        .child(text(label).font(theme::SM_MEDIUM).color(t.c.foreground).single_line().truncate().shrink().min_w(0.0))
        .children(version.map(|v| text(format!("v{v}")).font(theme::TEXT_SM).color(t.c.muted_foreground)));
    let copied = s.ui.chat.home.triage_copied.is_some_and(|at| data::now_ms() - at < TRIAGE_COPIED_MS);
    let mut copy = row()
        .key("home-triage-command")
        .center_y()
        .gap(4.0)
        .hover_group()
        .cursor(Cursor::Pointer)
        .focusable(true)
        .role("button")
        .label(if copied { "Copied".to_owned() } else { format!("or run {}", crate::chat::triage::CLI_COMMAND) })
        .help("Copy command, for a terminal")
        .on_press_tx(|s: &mut App, tx: &mut Tx| {
            tx.copy(crate::chat::triage::CLI_COMMAND);
            s.ui.chat.home.triage_copied = Some(data::now_ms());
            tx.wake_after(std::time::Duration::from_millis(TRIAGE_COPIED_MS as u64));
        })
        .child(text(if copied { "Copied" } else { "or run" }).font(theme::SM_MEDIUM).color(t.c.muted_foreground).group_color(t.c.foreground));
    if !copied {
        copy = copy.child(text(crate::chat::triage::CLI_COMMAND).font(theme::MONO_XS).color(t.c.muted_foreground).group_color(t.c.foreground));
    }
    row().gap(6.0).center_y().min_w(0.0).child(machine).child(text("·").font(theme::TEXT_SM).color(t.c.muted_foreground)).child(copy)
}

/// Whether the open draft still links the triage playbook (`isTriagePrompt`).
fn triage_draft(s: &App) -> bool {
    let key = composer::draft_key(s, s.frame_route());
    s.ui.chat.composer.drafts.get(&key).is_some_and(|d| crate::chat::triage::is_triage_prompt(&d.edit))
}

/// A loaded thread's empty prompt (MessagesTimeline's empty state) centred above the composer,
/// or the draft hero resting 16 points above the centred composer (ChatView's draft hero),
/// clear of anything docked on its shoulder.
pub fn view(s: &App, cx: &mut Cx<App>, t: &T, lane: &Lane) -> El {
    let composer_h = cx.layout_with("composer-lane", |r| r[3].round()).unwrap_or(112.0);
    let page = col().key("home").w_full().grow(1.0).min_h(0.0).pl(lane.left);
    if !composer::draft_hero(s) {
        let empty = text("Send a message to start the conversation.").font(theme::TEXT_SM).color(t.c.muted_foreground);
        return page.justify_center().pb(composer_h + composer::BOTTOM + 8.0).child(row().w(lane.width).justify_center().child(empty));
    }
    let project = draft_project(s);
    let scratch = project.is_some_and(|p| crate::shell::palette::actions::is_scratch_project(s, p));
    let can_choose = s.state().projects.iter().any(|p| p["archivedAt"].is_null());
    let hero = hero(project.map(|p| (data::s(p, "title"), scratch)), can_choose, scratch && triage_draft(s));
    let content = col().items_center().w_full().child(headline(t, &hero, lane.width)).children(subline(s, t, &hero).map(|line| row().h(24.0).mt(8.0).center_y().justify_center().child(line)));
    let card_h = cx.layout_with("composer", |r| r[3].round()).unwrap_or(composer_h);
    let above = hero_gap(card_h, composer_h);
    page.justify_end().pb(composer::bottom_inset(s, cx) + above).child(row().w(lane.width).justify_center().child(content))
}

/// From the composer card's bottom to the hero's: the card and `pb-4`, or whatever rides the
/// card's shoulder when that stands taller.
fn hero_gap(card_h: f32, lane_h: f32) -> f32 {
    (card_h + 16.0).max(lane_h)
}

/// Scenes: the draft "Triage an issue" opens, on this machine's scratch project.
pub fn triage_scene(s: &mut App) {
    if let data::Source::Fixture(state) = &mut s.src {
        let config = state.config.get_or_insert_with(|| json!({}));
        config["scratchWorkspaceRoot"] = json!("/home/dev/.t3/scratch");
        config["environment"]["serverVersion"] = json!("0.0.27");
        state.projects.push(json!({"id": "scene-scratch", "title": "Scratch", "workspaceRoot": "/home/dev/.t3/scratch"}));
    }
    let prompt = "Read [triage-playbook.md](/home/dev/.t3/userdata/triage/2026-11-22T10-00-00/triage-playbook.md) and follow it. ";
    crate::chat::triage::open_draft(s, "scene-scratch".into(), prompt);
    crate::scene_clock(s);
}

#[cfg(test)]
mod tests {
    use super::{Hero, hero, hero_gap, offers_scratch, triage_draft};

    #[test]
    fn the_triage_scene_shows_the_triage_hero_until_the_playbook_link_goes() {
        let mut s = crate::App::new();
        super::triage_scene(&mut s);
        let project = super::draft_project(&s).expect("the scratch project");
        assert!(crate::shell::palette::actions::is_scratch_project(&s, project));
        assert!(triage_draft(&s));
        let key = crate::chat::composer::draft_key(&s, &s.route);
        s.ui.chat.composer.drafts.get_mut(&key).unwrap().edit.clear();
        assert!(!triage_draft(&s), "a cleared draft asks what to work on");
    }

    #[test]
    fn the_hero_sits_16_points_over_the_card_or_over_its_shoulder() {
        assert_eq!(hero_gap(112.0, 112.0), 128.0);
        assert_eq!(hero_gap(112.0, 160.0), 160.0, "a docked notice or stash tab lifts the hero");
    }

    #[test]
    fn the_draft_hero_asks_about_the_drafts_project() {
        assert_eq!(hero(Some(("acme", false)), true, false), Hero::Project("acme".into()));
        assert_eq!(hero(Some(("Scratch", true)), true, false), Hero::Scratch);
        assert_eq!(hero(Some(("Scratch", true)), true, true), Hero::Triage);
        assert_eq!(hero(Some(("acme", false)), true, true), Hero::Project("acme".into()), "only a draft without a project triages");
        assert_eq!(hero(None, true, false), Hero::Choose);
        assert_eq!(hero(None, false, false), Hero::AddProject);
        assert!(!offers_scratch(&Hero::Triage, true));
        assert!(offers_scratch(&Hero::Project("acme".into()), true));
        assert!(!offers_scratch(&Hero::Project("acme".into()), false), "the server has no scratch workspace");
        assert!(!offers_scratch(&Hero::Scratch, true), "already without a project");
        assert!(!offers_scratch(&Hero::AddProject, true));
    }
}
