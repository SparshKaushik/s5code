//! Settings, as the web client's route modal: a 240 point nav of sections and a content pane of
//! grouped rows.

use rune::{Cursor, El, ScrollSpec, Tx, col, el, icon, row, spacer, text, text_input};
use rune_kit::{Kit, SegmentedControl, Select, Switch};

use crate::icons as i;
use crate::theme::{self, P};
use crate::ui::{self, icon_button};
use crate::{App, Appearance, MODELS};

type Item = (&'static str, &'static rune::Icon);
const NAV: &[(Option<&str>, &[Item])] = &[
    (None, &[("General", &i::SETTINGS), ("Appearance", &i::SUN), ("Chat", &i::MESSAGE), ("Diffs & pull requests", &i::PULL_REQUEST), ("Keybindings", &i::KEYBOARD), ("SnapShots", &i::CAMERA)]),
    (Some("Agents"), &[("Providers", &i::BOT), ("Threads", &i::MESSAGE), ("Browser", &i::GLOBE), ("Devices", &i::SERVER), ("Scheduled tasks", &i::CLOCK)]),
    (Some("Code"), &[("Projects", &i::FOLDER), ("Source control", &i::GIT_BRANCH), ("Worktrees", &i::GIT_BRANCH)]),
    (Some("Connections"), &[("Environments", &i::SERVER), ("Sharing & access", &i::USERS)]),
    (None, &[("Maintenance", &i::WRENCH), ("Labs", &i::FLASK), ("About", &i::INFO), ("Archive", &i::ARCHIVE)]),
];

fn name(section: usize) -> &'static str {
    NAV.iter().flat_map(|g| g.1.iter()).nth(section).map_or("General", |x| x.0)
}

pub fn view(s: &App, section: usize, p: P, k: &Kit) -> Vec<El> {
    let mut pops = Vec::new();
    let panel = row()
        .key("settings-dialog")
        .role("dialog")
        .label("Settings")
        .w_full()
        .h_full()
        .max_w(1280.0)
        .max_h(960.0)
        .radius(16.0)
        .fill(p.bg)
        .border(1.0, p.border)
        .shadow(24.0, 64.0, rune::Color::BLACK.alpha(0.5))
        .clip_rounded(16.0)
        .modal(true, |s: &mut App| {
            s.settings = None;
            s.open_select = None;
        })
        .child(nav(s, section, p))
        .child(content(s, section, p, k, &mut pops));
    let mut out = vec![
        el().key("settings-scrim").absolute(0.0, 0.0).w_full().h_full().fill(p.scrim).a11y_hidden(),
        el().key("settings-layer").absolute(0.0, 0.0).w_full().h_full().padding(40.0).center().child(panel),
    ];
    out.extend(pops);
    out
}

fn nav(s: &App, section: usize, p: P) -> El {
    let search = row()
        .key("settings-search")
        .h(32.0)
        .w_full()
        .px(8.0)
        .gap(6.0)
        .center_y()
        .radius(6.0)
        .border(1.0, p.input)
        .child(icon(&i::SEARCH, 14.0).color(p.muted))
        .child(text_input(&s.settings_search, theme::SM).single_line().placeholder("Search", p.faint).color(p.fg).grow(1.0).shrink().min_w(0.0).on_edit(|s: &mut App| &mut s.settings_search));
    let mut c = col().key("settings-nav").w(240.0).h_full().fill(p.card).px(8.0).pt(14.0).gap(1.0).child(search.mb(10.0));
    let mut n = 0;
    for (heading, items) in NAV {
        if let Some(h) = heading {
            c = c.child(text(h.to_string()).font(theme::XS_MEDIUM).color(p.faint).px(10.0).pt(12.0).pb(4.0));
        } else if n > 0 {
            c = c.child(el().h(10.0));
        }
        for (label, glyph) in items.iter() {
            let idx = n;
            let on = idx == section;
            let mut r = row()
                .key(format!("nav-{idx}"))
                .role("tab")
                .label(label.to_string())
                .w_full()
                .h(30.0)
                .px(10.0)
                .gap(8.0)
                .center_y()
                .radius(6.0)
                .hover(p.row_hover)
                .cursor(Cursor::Pointer)
                .on_press(move |s: &mut App| {
                    s.settings = Some(idx);
                    s.open_select = None;
                })
                .child(icon(glyph, 15.0).color(if on { p.fg } else { p.muted }))
                .child(text(label.to_string()).font(theme::SM).color(if on { p.fg } else { p.muted }));
            if on {
                r = r.fill(p.row_active);
            }
            c = c.child(r);
            n += 1;
        }
    }
    c.child(el().absolute(239.0, 0.0).w(1.0).h_full().fill(p.border))
}

fn content(s: &App, section: usize, p: P, k: &Kit, pops: &mut Vec<El>) -> El {
    let title = name(section);
    let mut head = row()
        .key("settings-head")
        .w_full()
        .h(48.0)
        .px(16.0)
        .gap(8.0)
        .center_y()
        .child(text("Settings").font(theme::SM).color(p.muted))
        .child(icon(&i::CHEVRON_RIGHT, 14.0).color(p.faint))
        .child(text(title).font(theme::SM_MEDIUM).color(p.fg))
        .child(spacer());
    if section == 0 {
        head = head.child(row().key("restore").h(28.0).px(10.0).center_y().radius(6.0).hover(p.row_hover).cursor(Cursor::Pointer).child(text("Restore device defaults").font(theme::XS).color(p.muted)));
    }
    head = head.child(icon_button(p, "settings-close", &i::X, 28.0, "Close settings", |s, _| s.settings = None));
    let groups: Vec<El> = match title {
        "General" => general(s, p, k, pops),
        "Appearance" => appearance(s, p, k),
        "Providers" => providers(s, p, k, pops),
        "Browser" => browser(s, p),
        _ => vec![group(p, "Coming soon", vec![row_item(p, title, "This section isn't part of the Rune prototype yet.", el())])],
    };
    let body = col().key("settings-body").w_full().max_w(896.0).px(32.0).pt(8.0).pb(40.0).gap(28.0).children(groups);
    col()
        .key("settings-content")
        .grow(1.0)
        .shrink()
        .min_w(0.0)
        .h_full()
        .child(head)
        .child(ui::hairline(p))
        .child(col().key("settings-scroll").w_full().grow(1.0).min_h(0.0).items_center().child(body).scroll(ScrollSpec { stick_bottom: false }))
}

fn group(p: P, title: &str, rows: Vec<El>) -> El {
    let mut card = col().w_full().radius(12.0).border(1.0, p.border).fill(p.card.alpha(0.6));
    let n = rows.len();
    for (j, r) in rows.into_iter().enumerate() {
        card = card.child(r);
        if j + 1 < n {
            card = card.child(ui::hairline(p));
        }
    }
    col().key(format!("group-{}", title.to_lowercase().replace(' ', "-"))).w_full().gap(10.0).child(text(title.to_string()).font(theme::BASE_SEMIBOLD).color(p.fg)).child(card)
}

fn row_item(p: P, title: &str, description: &str, control: El) -> El {
    row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(24.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .gap(2.0)
                .child(text(title.to_string()).font(theme::SM_MEDIUM).color(p.fg))
                .child(text(description.to_string()).font(theme::XS).color(p.muted).wrap()),
        )
        .child(row().min_w(160.0).justify_end().child(control))
}

fn switch(k: &Kit, on: bool, label: &str, f: fn(&mut App)) -> El {
    Switch::new(on).label(label.to_string()).hide_label().on_toggle(f).build(k)
}

fn select(s: &App, k: &Kit, pops: &mut Vec<El>, key: &'static str, options: &[&str], field: fn(&mut App) -> &mut String, current: &str) -> El {
    Select::new(key, options)
        .appearance(rune_kit::SelectAppearance::Field)
        .width(200.0)
        .current(Some(current))
        .open(s.open_select == Some(key))
        .on_toggle(move |s: &mut App| s.open_select = if s.open_select == Some(key) { None } else { Some(key) })
        .on_close(|s: &mut App| s.open_select = None)
        .on_choose(move |s: &mut App, v: &str, _: &mut Tx| {
            *field(s) = v.to_string();
            s.open_select = None;
        })
        .build(k, pops)
}

fn general(s: &App, p: P, k: &Kit, pops: &mut Vec<El>) -> Vec<El> {
    let pr = &s.prefs;
    vec![
        group(p, "Notifications", vec![
            row_item(p, "Thread notifications", "System alerts when a thread finishes, fails, or needs input or approval. Applies to this device while S5 Code is open.",
                select(s, k, pops, "thread-notifications", &["Off", "Notifications only", "Sound only", "Notifications with sound"], |s| &mut s.prefs.thread_notifications, &pr.thread_notifications)),
            row_item(p, "In-app notifications", "Show a toast when another thread finishes, fails, or needs input or approval while this app has focus.",
                switch(k, pr.in_app_notifications, "In-app notifications", |s| s.prefs.in_app_notifications ^= true)),
        ]),
        group(p, "Sidebar", vec![
            row_item(p, "Project grouping", "Combine matching repositories across environments.",
                switch(k, pr.project_grouping, "Project grouping", |s| s.prefs.project_grouping ^= true)),
            row_item(p, "Project order", "Order of projects in the sidebar project picker and command palette.",
                select(s, k, pops, "project-order", &["Recent activity", "Alphabetical", "Manual"], |s| &mut s.prefs.project_order, &pr.project_order)),
            row_item(p, "Working section", "Fold working and monitoring threads into a Working section. They return to the top of the inbox when they need you. The boring sidebar in Labs has no Working section.",
                switch(k, pr.working_section, "Working section", |s| s.prefs.working_section ^= true)),
        ]),
        group(p, "Confirmations", vec![
            row_item(p, "Unpin confirmation", "Ask before unpinning a thread from the pinned section.", switch(k, pr.unpin_confirm, "Confirm thread unpinning", |s| s.prefs.unpin_confirm ^= true)),
            row_item(p, "Archive confirmation", "Require a second click on the inline archive action before a thread is archived.", switch(k, pr.archive_confirm, "Confirm thread archiving", |s| s.prefs.archive_confirm ^= true)),
            row_item(p, "Delete confirmation", "Ask before deleting a thread and its chat history.", switch(k, pr.delete_confirm, "Confirm thread deletion", |s| s.prefs.delete_confirm ^= true)),
            row_item(p, "Quit shortcut", "Hold mode also quits on two quick presses.",
                select(s, k, pops, "quit-shortcut", &["Hold to quit", "Press twice to quit", "Quit immediately"], |s| &mut s.prefs.quit_shortcut, &pr.quit_shortcut)),
        ]),
        group(p, "Display", vec![row_item(p, "Time format", "System default follows your browser or OS clock preference.",
            select(s, k, pops, "time-format", &["System default", "12-hour", "24-hour"], |s| &mut s.prefs.time_format, &pr.time_format))]),
    ]
}

fn appearance(s: &App, p: P, k: &Kit) -> Vec<El> {
    let at = match s.appearance {
        Appearance::System => 0,
        Appearance::Light => 1,
        Appearance::Dark => 2,
    };
    let theme_picker = SegmentedControl::new(&["System", "Light", "Dark"], Some(at))
        .label("Theme")
        .on_pick(|s: &mut App, n, tx: &mut Tx| {
            tx.color_motion(rune::Spring::new(0.4, 0.0));
            s.appearance = [Appearance::System, Appearance::Light, Appearance::Dark][n];
        })
        .build(k);
    vec![group(p, "Theme", vec![
        row_item(p, "Color scheme", "Follow the system, or pick light or dark. Rune springs every colour across the switch.", theme_picker),
        row_item(p, "Reduce motion", "Snap layout changes instead of animating them.", switch(k, s.prefs.reduce_motion, "Reduce motion", |s| s.prefs.reduce_motion ^= true)),
    ])]
}

fn providers(s: &App, p: P, k: &Kit, pops: &mut Vec<El>) -> Vec<El> {
    let (dot, state) = match s.connected {
        Some(true) => (p.success, "Connected"),
        Some(false) => (p.error, "Not reachable"),
        None => (p.faint, "Checking…"),
    };
    let names: Vec<&str> = MODELS.iter().map(|m| m.1).collect();
    let current = MODELS.iter().find(|m| m.0 == s.model).map_or("", |m| m.1);
    let model = Select::new("default-model", &names)
        .appearance(rune_kit::SelectAppearance::Field)
        .width(220.0)
        .current(Some(current))
        .open(s.open_select == Some("default-model"))
        .on_toggle(|s: &mut App| s.open_select = if s.open_select == Some("default-model") { None } else { Some("default-model") })
        .on_close(|s: &mut App| s.open_select = None)
        .on_choose(|s: &mut App, v: &str, _: &mut Tx| {
            if let Some(m) = MODELS.iter().find(|m| m.1 == v) {
                s.model = m.0.into();
            }
            s.open_select = None;
        })
        .build(k, pops);
    let status = row().gap(6.0).center_y().child(el().size(8.0, 8.0).radius(4.0).fill(dot)).child(text(state).font(theme::SM).color(p.fg));
    let unsupported = |name: &str| row_item(p, name, "Not wired up in the Rune prototype; turns run through OpenCode.", text("Off").font(theme::SM).color(p.faint));
    vec![
        group(p, "OpenCode", vec![
            row_item(p, "Server", &std::env::var("S5_OPENCODE_URL").unwrap_or_else(|_| "http://127.0.0.1:4096".into()), status),
            row_item(p, "Default model", "Free OpenCode Zen models; the composer's picker overrides it per thread.", model),
        ]),
        group(p, "Other providers", vec![unsupported("Codex"), unsupported("Claude"), unsupported("Cursor"), unsupported("Grok")]),
    ]
}

fn browser(s: &App, p: P) -> Vec<El> {
    let state = match (&s.page.error, s.page.frames) {
        (Some(e), _) => e.clone(),
        (None, 0) => "Idle".into(),
        (None, n) => format!("Rendering · {n} frames"),
    };
    vec![group(p, "Preview engine", vec![
        row_item(p, "Renderer", "Headless Chrome over the DevTools protocol; frames stream into a Rune pixel surface.", text(state).font(theme::SM).color(p.muted)),
        row_item(p, "Current page", if s.page.url.is_empty() { "Nothing open" } else { &s.page.url }, text(s.page.title.clone()).font(theme::SM).color(p.muted).truncate().max_w(240.0)),
    ])]
}
