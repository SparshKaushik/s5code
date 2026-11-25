//! Palette scenes over the fixtures; replies a live server would send are injected directly.
use super::{actions, add_project, push_page, set_query};
use crate::{App, Route, data};
use rune::TextEdit;
use serde_json::{Value, json};

pub const SCENES: &[(&str, fn(&mut App))] = &[
    ("palette-projects-grouped", |s| {
        open_thread(s);
        if let data::Source::Fixture(state) = &mut s.src
            && let Some(first) = state.projects.first_mut()
        {
            first["repositoryIdentity"] = json!({"canonicalKey": "github.com/acme/s5code", "name": "s5code"});
            let mut copy = first.clone();
            copy["id"] = json!("p-box-copy");
            copy["workspaceRoot"] = json!("/srv/s5code");
            copy[crate::env::TAG] = json!("box");
            state.projects.push(copy);
        }
        s.ui.env.saved.push(crate::env::Saved { id: "box".into(), label: "Build box".into(), base: "https://box.example.com".into(), bearer: String::new() });
        open(s);
        push_page(s, "projects", "");
    }),
    ("palette-actions-pr", |s| {
        open_thread(s);
        let thread = current(s);
        if let data::Source::Fixture(state) = &mut s.src
            && let Some(row) = state.threads.iter_mut().find(|v| v["id"] == thread)
        {
            row["pullRequests"] = json!([{
                "host": "github.com", "repository": "acme/s5code", "number": 412,
                "url": "https://github.com/acme/s5code/pull/412", "source": "created",
                "linkedAt": "2026-11-06T09:00:00.000Z", "stack": null,
                "snapshot": {"state": "open", "title": "Replay showcase", "headBranch": "t3/replay", "baseBranch": "main",
                    "isDraft": false, "updatedAt": "2026-11-06T09:10:00.000Z", "syncedAt": "2026-11-06T09:10:00.000Z"},
            }]);
        }
        open(s);
    }),
    ("palette-add-project", |s| {
        open_thread(s);
        open(s);
        s.ui.shell.palette.add.discovery = Some(discovery());
        add_project::open(s);
    }),
    ("palette-browse", |s| {
        open_thread(s);
        open(s);
        push_page(s, "add-project", "");
        push_page(s, "add-browse", "~/projects/");
        browse(s, "~/projects/", "/home/ubuntu/projects", &["billing-api", "rune-site", "s5code", "widgets", ".cache"]);
    }),
    ("palette-browse-denied", |s| {
        open_thread(s);
        deny_files(s);
        open(s);
        push_page(s, "add-project", "");
        push_page(s, "add-browse", "~/projects/");
    }),
    ("content-search-results", |s| {
        open_thread(s);
        crate::search::scene_results(s);
    }),
    ("content-search-long", |s| {
        open_thread(s);
        crate::search::scene_long(s);
    }),
    ("content-search-error", |s| {
        open_thread(s);
        crate::search::scene_error(s);
    }),
    ("content-search-denied", |s| {
        open_thread(s);
        deny_files(s);
        crate::search::open(s);
    }),
    ("palette-browse-filter", |s| {
        open_thread(s);
        open(s);
        push_page(s, "add-project", "");
        push_page(s, "add-browse", "~/projects/new-app");
        browse(s, "~/projects/", "/home/ubuntu/projects", &["billing-api", "rune-site", "s5code", "widgets"]);
    }),
    ("palette-clone-repo", |s| {
        open_thread(s);
        open(s);
        s.ui.shell.palette.add.discovery = Some(discovery());
        add_project::open(s);
        add_project::choose(s, add_project::Source::Clone(Some(0)));
        set_query(s, "acme/widgets");
    }),
    ("palette-clone-confirm", |s| {
        open_thread(s);
        open(s);
        add_project::open(s);
        add_project::choose(s, add_project::Source::Clone(Some(0)));
        add_project::confirm(s, add_project::Confirm {
            input: "acme/widgets".into(),
            name_with_owner: Some("acme/widgets".into()),
            url: "https://github.com/acme/widgets".into(),
            remote_url: "https://github.com/acme/widgets.git".into(),
        });
        browse(s, "~/", "/home/ubuntu", &["projects", "s5home", "widgets", "notes"]);
    }),
    ("palette-new-project", |s| {
        open_thread(s);
        open(s);
        s.ui.shell.palette.add.discovery = Some(discovery());
        add_project::open(s);
        add_project::choose(s, add_project::Source::New);
        set_query(s, "My App");
        s.ui.shell.palette.add.publish = true;
    }),
    ("palette-add-environment", |s| {
        open_thread(s);
        park_box(s);
        open(s);
        add_project::open(s);
    }),
    ("palette-new-project-machines", |s| {
        open_thread(s);
        park_box(s);
        open(s);
        // Starting picks the first environment, which drops any discovery from before.
        add_project::start_new(s);
        s.ui.shell.palette.add.discovery = Some(discovery());
        set_query(s, "My App");
    }),
    ("palette-themes", |s| {
        open_thread(s);
        open(s);
        push_page(s, "themes", "");
    }),
    ("palette-themes-published", |s| {
        open_thread(s);
        // Seeded published themes (`createVividThemeColors`); the app wears the dark one.
        if let data::Source::Fixture(state) = &mut s.src {
            state.config.get_or_insert_with(|| json!({}))["environmentThemes"] = json!([
                {"id": "ember", "name": "Ember", "appearance": "dark", "canvas": "#17110d", "accent": "#f97316"},
                {"id": "tide", "name": "Tide", "appearance": "light", "canvas": "#f3f8fb", "accent": "#0e7490", "variants": {"dark": {}}},
            ]);
        }
        s.client_settings.values.insert("themeDark".into(), json!("ember"));
        open(s);
        push_page(s, "themes", "");
    }),
    ("palette-thread-rows", |s| {
        open_thread(s);
        decorate_threads(s);
        open(s);
    }),
    ("palette-thread-pr", |s| {
        open_thread(s);
        decorate_threads(s);
        open(s);
        set_query(s, "#412");
    }),
    ("palette-linked", |s| {
        open_thread(s);
        let thread = current(s);
        let threads = vec![
            json!({"id": thread, "title": "Replay showcase", "archivedAt": null}),
            json!({"id": "t-retry", "title": "Retry webhook deliveries", "archivedAt": "2026-11-01T00:00:00.000Z"}),
        ];
        super::open_linked(s, "https://github.com/acme/s5code/pull/412", crate::env::LOCAL, Some(threads));
    }),
    ("keybindings-warning", |s| {
        open_thread(s);
        actions::keybindings_warning(s, "Invalid keybinding at index 2: unknown command \"thread.jump.0\".");
    }),
    ("palette-files", |s| {
        open_thread(s);
        open(s);
        push_page(s, "files", "");
        set_query(s, "pal");
        let files = &mut s.ui.shell.palette.act.files;
        files.entries = [
            "apps/desktop-rune/src/shell/palette/mod.rs",
            "apps/desktop-rune/src/shell/palette/actions.rs",
            "apps/web/src/components/CommandPalette.tsx",
            "apps/web/src/components/CommandPalette.logic.ts",
            "packages/shared/src/paletteSearch.ts",
        ]
        .map(str::to_owned)
        .to_vec();
        files.sent = Some("pal".into());
        files.loaded = Some("pal".into());
    }),
    ("palette-link-pr", |s| {
        open_thread(s);
        actions::open_link(s, current(s));
        if let Some(dialog) = &mut s.ui.shell.palette.act.link {
            dialog.input = TextEdit::new("https://github.com/acme/s5code/pull/412");
            dialog.input.focused = true;
            dialog.dirty = true;
        }
    }),
    ("palette-link-pr-error", |s| {
        open_thread(s);
        actions::open_link(s, current(s));
        if let Some(dialog) = &mut s.ui.shell.palette.act.link {
            dialog.input = TextEdit::new("42");
            dialog.input.focused = true;
            dialog.dirty = true;
        }
    }),
];

fn open_thread(s: &mut App) {
    crate::scene_clock(s);
    if let Some(id) = s.state().threads.iter().find(|t| t["title"] == "Replay showcase").map(|t| data::s(t, "id").to_owned()) {
        s.navigate(Route::Thread(id));
    }
}
/// A session without `filesystem:read`, as a connection paired with read-only access has.
fn deny_files(s: &mut App) {
    let key = s.ui.env.active.clone();
    crate::settings::permissions::set_session(s, &key, json!({"authenticated": true, "permissions": ["orchestration:read"]}));
}
fn current(s: &App) -> String {
    match &s.route {
        Route::Thread(id) => id.clone(),
        _ => String::new(),
    }
}
/// Statuses, a pull request and a worktree on the fixture's threads, as the palette rows show them.
fn decorate_threads(s: &mut App) {
    let current = current(s);
    let data::Source::Fixture(state) = &mut s.src else { return };
    let mut others = state.threads.iter_mut().filter(|t| t["id"] != current.as_str());
    let mut extend = |extra: Value| {
        if let Some(t) = others.next().and_then(Value::as_object_mut) {
            t.extend(extra.as_object().cloned().unwrap_or_default());
        }
    };
    extend(json!({"status": "running", "latestRunId": "run-1"}));
    extend(json!({"pendingRuntimeRequest": {"kind": "approval"}}));
    extend(json!({
        "branch": "feature/retry-webhooks",
        "worktreePath": "/src/s5code/.t3/worktrees/retry",
        "pullRequests": [{"number": 412, "url": "https://github.com/acme/s5code/pull/412", "snapshot": {"state": "open", "title": "Retry webhook deliveries"}}],
    }));
    extend(json!({"status": "idle", "latestRunId": "run-2", "latestRunCompletedAt": "2026-11-20T10:00:00.000Z", "lastVisitedAt": "2026-11-19T10:00:00.000Z"}));
}
fn open(s: &mut App) {
    super::reset(s, true);
}
/// A second connected machine, "Build box", that can take new projects.
fn park_box(s: &mut App) {
    let mut state = data::empty_state();
    state.conn = s5_net::Conn::Open;
    state.shell_ready = true;
    state.config = Some(json!({"newProjectsRoot": "/srv/projects", "environment": {"label": "Build box"}}));
    s.ui.env.saved.push(crate::env::Saved { id: "box".into(), label: "Build box".into(), base: "https://box.example.com".into(), bearer: String::new() });
    crate::env::park(s, "box", data::Source::Fixture(state));
}
fn browse(s: &mut App, directory: &str, parent: &str, names: &[&str]) {
    let entries: Vec<Value> = names.iter().map(|name| json!({"name": name, "fullPath": format!("{parent}/{name}")})).collect();
    s.ui.shell.palette.add.browse = Some(add_project::Browse {
        directory: directory.into(),
        pending: None,
        result: Some(json!({"parentPath": parent, "entries": entries})),
        error: None,
    });
}
fn discovery() -> Value {
    let provider = |kind: &str, label: &str, status: &str, auth: &str, account: Option<&str>| {
        json!({
            "kind": kind, "label": label, "status": status,
            "installHint": format!("Install the {label} CLI to clone {label} repositories."),
            "version": {"_tag": "None"}, "detail": {"_tag": "None"},
            "auth": {
                "status": auth,
                "account": account.map_or(json!({"_tag": "None"}), |a| json!({"_tag": "Some", "value": a})),
                "host": {"_tag": "None"}, "detail": {"_tag": "None"},
            },
        })
    };
    json!({
        "versionControlSystems": [],
        "sourceControlProviders": [
            provider("github", "GitHub", "available", "authenticated", Some("sparsh")),
            provider("gitlab", "GitLab", "available", "unauthenticated", None),
            provider("gitcafe", "GitCafe", "missing", "unknown", None),
            provider("forgejo", "Forgejo / Gitea", "available", "authenticated", None),
            provider("bitbucket", "Bitbucket", "missing", "unknown", None),
            provider("azure-devops", "Azure DevOps", "missing", "unknown", None),
        ],
    })
}
