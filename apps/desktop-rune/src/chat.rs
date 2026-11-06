//! The main column: the header breadcrumb, the draft hero ("What should we build in …?"), the
//! transcript and the composer.

use rune::{Cursor, Cx, El, ScrollSpec, col, el, icon, row, spacer, text, text_input};
use rune_kit::{Kit, Markdown, Select, Spinner};

use crate::icons as i;
use crate::opencode::Part;
use crate::theme::{self, P};
use crate::ui::{self, icon_button};
use crate::{App, MODELS, Route, Status, Thread};

const CONTENT_MAX: f32 = 736.0;

pub fn view(s: &App, _cx: &mut Cx<App>, p: P, k: &Kit, layer: &mut Vec<El>) -> El {
    let body = match s.route {
        Route::Draft(project) => draft(s, project, p, k, layer),
        Route::Thread(id) => match s.thread(id) {
            Some(t) => thread(s, t, p, k, layer),
            None => draft(s, 0, p, k, layer),
        },
    };
    col().key("main").grow(1.0).shrink().min_w(0.0).h_full().fill(p.bg).child(header(s, p)).child(body)
}

fn header(s: &App, p: P) -> El {
    let project = &s.projects[s.current_project()];
    let title = match s.route {
        Route::Draft(_) => "New thread".to_string(),
        Route::Thread(id) => s.thread(id).map(|t| t.title.clone()).unwrap_or_default(),
    };
    let crumb = row()
        .key("breadcrumb")
        .gap(8.0)
        .center_y()
        .shrink()
        .min_w(0.0)
        .child(ui::favicon(&project.name, project.color, 14.0))
        .child(text(project.name.clone()).font(theme::SM).color(p.muted).max_w(160.0).truncate())
        .child(text("/").font(theme::SM).color(p.faint))
        .child(text(title).font(theme::SM_MEDIUM).color(p.fg).truncate().shrink())
        .child(icon(&i::CHEVRON_DOWN, 14.0).color(p.faint));
    let panel_tint = if s.panel_open { p.fg } else { p.muted };
    row()
        .key("chat-header")
        .w_full()
        .h(52.0)
        .px(20.0)
        .gap(4.0)
        .center_y()
        .window_drag()
        .child(crumb)
        .child(spacer())
        .child(icon_button(p, "toggle-details", &i::SQUARE_MENU, 28.0, "Toggle thread details panel", |_, _| {}))
        .child(icon_button(p, "toggle-terminal", &i::PANEL_BOTTOM, 28.0, "Toggle terminal drawer", |_, _| {}))
        .child(ui::icon_button_tinted(p, "toggle-panel", &i::PANEL_RIGHT, 28.0, "Toggle right panel", panel_tint, |s, _| s.panel_open = !s.panel_open))
}

fn draft(s: &App, project: usize, p: P, k: &Kit, layer: &mut Vec<El>) -> El {
    let name = s.projects.get(project).map_or("your project".to_string(), |p| p.name.clone());
    let headline = row()
        .key("hero-headline")
        .gap(9.0)
        .center()
        .child(text("What should we build in").font(theme::HERO).color(p.fg))
        .child(
            col()
                .key("hero-project")
                .cursor(Cursor::Pointer)
                .on_press(move |s: &mut App| s.route = Route::Draft((project + 1) % s.projects.len()))
                .child(text(format!("{name}?")).font(rune::tok::TextStyle { weight: 500.0, ..theme::HERO }).color(p.fg))
                .child(el().w_full().h(1.0).fill(p.muted.alpha(0.6))),
        );
    let sub = text("or start without a project").key("hero-sub").font(theme::SM).color(p.muted).mt(6.0);
    col()
        .key("draft")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .px(24.0)
        .pb(52.0)
        .center()
        .child(col().w_full().max_w(CONTENT_MAX).items_center().pb(16.0).child(headline).child(sub))
        .child(composer(s, p, k, layer, false))
}

fn thread(s: &App, t: &Thread, p: P, k: &Kit, layer: &mut Vec<El>) -> El {
    let mut rows = col().key("rows").w_full().max_w(CONTENT_MAX).gap(0.0).px(4.0).pt(16.0);
    if !t.loaded || (t.messages.is_empty() && t.session.is_some() && !matches!(t.status, Status::Working(_))) {
        rows = rows.child(row().gap(8.0).center_y().py(8.0).child(Spinner::new().size(14.0).color(p.muted).build(k)).child(text("Loading thread…").font(theme::SM).color(p.muted)));
    }
    for (n, m) in t.messages.iter().enumerate() {
        if m.user {
            let body = m.parts.iter().filter_map(|x| if let Part::Text(t) = x { Some(t.as_str()) } else { None }).collect::<Vec<_>>().join("\n");
            rows = rows.child(
                row().key(format!("msg-{n}")).w_full().justify_end().pb(16.0).child(
                    col().max_w(560.0).padding(12.0).radius(16.0).fill(p.message).child(text(body).font(theme::SM.lh(22.0)).color(p.fg).wrap().selectable(true)),
                ),
            );
            continue;
        }
        let mut c = col().key(format!("msg-{n}")).w_full().gap(4.0).pb(16.0);
        for (j, part) in m.parts.iter().enumerate() {
            c = c.child(match part {
                Part::Text(body) => col().key(format!("md-{j}")).w_full().px(4.0).child(Markdown::new(body.clone()).selectable(true).build(k)),
                Part::Reasoning(r) => work_row(p, k, &format!("work-{j}"), &i::BRAIN, "Thinking", first_line(r), true),
                Part::Tool { name, title, done } => work_row(p, k, &format!("work-{j}"), tool_icon(name), name, title.clone(), *done),
            });
        }
        if let Some(e) = &m.error {
            c = c.child(row().key("error").gap(6.0).px(4.0).center_y().child(icon(&i::INFO, 14.0).color(p.error)).child(text(e.clone()).font(theme::SM).color(p.error).wrap()));
        }
        rows = rows.child(c);
    }
    if let Status::Working(since) = t.status {
        rows = rows.child(
            row()
                .key("working")
                .gap(8.0)
                .px(4.0)
                .pb(16.0)
                .center_y()
                .child(Spinner::new().size(14.0).color(p.info).build(k))
                .child(text(format!("Working for {}", ui::duration(s.now_ms, since))).font(theme::SM).color(p.muted)),
        );
    }
    let transcript = col()
        .key("transcript")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(row().w_full().justify_center().px(24.0).child(rows))
        .scroll(ScrollSpec { stick_bottom: true })
        .scroll_edge_fade(16.0);
    col()
        .key("thread")
        .w_full()
        .grow(1.0)
        .min_h(0.0)
        .child(transcript)
        .child(row().w_full().justify_center().px(24.0).pb(16.0).child(composer(s, p, k, layer, true)))
}

fn first_line(s: &str) -> String {
    s.lines().find(|l| !l.trim().is_empty()).unwrap_or("").chars().take(120).collect()
}

fn tool_icon(name: &str) -> &'static rune::Icon {
    match name {
        "bash" => &i::TERMINAL,
        "read" | "write" | "edit" | "glob" | "grep" | "list" => &i::FILE_TEXT,
        "webfetch" => &i::GLOBE,
        _ => &i::WRENCH,
    }
}

fn work_row(p: P, k: &Kit, key: &str, glyph: &'static rune::Icon, label: &str, detail: String, done: bool) -> El {
    let lead = if done { icon(glyph, 16.0).color(p.muted) } else { Spinner::new().size(14.0).color(p.muted).build(k) };
    row()
        .key(key.to_string())
        .w_full()
        .min_h(24.0)
        .px(4.0)
        .gap(6.0)
        .center_y()
        .child(row().size(16.0, 16.0).center().child(lead))
        .child(text(label.to_string()).font(theme::SM).color(p.secondary))
        .child(text(detail).font(theme::SM).color(p.faint).truncate().shrink().min_w(0.0))
}

fn composer(s: &App, p: P, k: &Kit, layer: &mut Vec<El>, docked: bool) -> El {
    let working = matches!(s.route, Route::Thread(id) if s.is_working(id));
    let model_label = MODELS.iter().find(|m| m.0 == s.model).map_or(s.model.as_str(), |m| m.1);
    let names: Vec<&str> = MODELS.iter().map(|m| m.1).collect();
    let picker = Select::new("model-picker", &names)
        .current(Some(model_label))
        .open(s.open_select == Some("model"))
        .on_toggle(|s: &mut App| s.open_select = if s.open_select == Some("model") { None } else { Some("model") })
        .on_close(|s: &mut App| s.open_select = None)
        .on_choose(|s: &mut App, v: &str, _| {
            if let Some(m) = MODELS.iter().find(|m| m.1 == v) {
                s.model = m.0.into();
            }
            s.open_select = None;
        })
        .build(k, layer);
    let send = if working {
        row().key("stop").role("button").label("Stop generation").size(32.0, 32.0).center().radius(16.0).fill(p.error).cursor(Cursor::Pointer).child(icon(&i::STOP, 14.0).color(rune::Color::WHITE))
    } else {
        let empty = s.composer.text.trim().is_empty();
        row()
            .key("send")
            .role("button")
            .label("Send")
            .size(32.0, 32.0)
            .center()
            .radius(16.0)
            .fill(if empty { p.primary.alpha(0.45) } else { p.primary })
            .cursor(Cursor::Pointer)
            .on_press_tx(|s: &mut App, tx| s.send(tx))
            .child(icon(&i::ARROW_UP, 16.0).color(p.on_primary))
    };
    let field = text_input(&s.composer, theme::SM.lh(22.0))
        .key("composer-input")
        .placeholder("Ask anything. @ files, $ skills, / commands", p.faint)
        .color(p.fg)
        .w_full()
        .min_h(44.0)
        .max_h(208.0)
        .on_edit(|s: &mut App| &mut s.composer)
        .on_submit(|s: &mut App, tx| s.send(tx));
    let footer = row()
        .key("composer-footer")
        .w_full()
        .px(8.0)
        .pb(8.0)
        .gap(2.0)
        .center_y()
        .child(icon_button(p, "attach", &i::PAPERCLIP, 28.0, "Attach files", |_, _| {}))
        .child(spacer())
        .child(ui::chip(p, "mode", Some(&i::BOT), "Build"))
        .child(ui::chip(p, "permissions", Some(&i::SHIELD), "Supervised"))
        .child(row().key("model").gap(2.0).pl(6.0).center_y().child(row().size(16.0, 16.0).center().radius(4.0).fill(p.fg.alpha(0.12)).child(text("oc").font(rune::tok::TextStyle::new(8.0, 700.0).lh(10.0)).color(p.muted))).child(picker))
        .child(el().w(4.0))
        .child(send);
    let mut c = col()
        .key(if docked { "composer-docked" } else { "composer" })
        .w_full()
        .max_w(CONTENT_MAX)
        .radius(16.0)
        .fill(p.card)
        .border(1.0, if p.dark { rune::Color::WHITE.alpha(0.07) } else { rune::Color::BLACK.alpha(0.08) })
        .child(col().w_full().px(16.0).pt(14.0).pb(6.0).child(field))
        .child(footer);
    if !p.dark {
        c = c.shadow(4.0, 16.0, rune::Color::BLACK.alpha(0.06));
    }
    c
}
