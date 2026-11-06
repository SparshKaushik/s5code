use std::sync::OnceLock;
use rune::{Cx, El, col, el, row, text};
use serde_json::{Value,json};
use crate::{App, icons};
use crate::theme::{self,T};
use super::{controls as c, request};

pub fn specs() -> &'static Value {
    static PAGES: OnceLock<Value> = OnceLock::new();
    PAGES.get_or_init(|| serde_json::from_str(include_str!("pages.json")).unwrap())
}

pub fn view(s: &App, _cx: &mut Cx<App>, t: &T, path: &str) -> El {
    let mut page = col().w_full().max_w(896.0).px(24.0).pt(24.0).pb(48.0).gap(32.0);
    if let Some(error) = s.ui.settings.error.as_ref().or(s.client_settings.error.as_ref()) {
        page = page.child(text(error.clone()).font(theme::TEXT_SM).color(t.c.destructive));
    }
    if path == "appearance" { page = page.child(themes(s,t)); }
    if let Some(sections) = specs()[path].as_array() {
        for section in sections {
            let title = section["title"].as_str().unwrap_or("");
            if title == "Advanced" {
                page = page.child(c::button(if s.ui.settings.advanced {"⌄ Advanced"}else{"› Advanced"},t)
                    .on_press(|s: &mut App| s.ui.settings.advanced = !s.ui.settings.advanced));
                if !s.ui.settings.advanced { continue; }
            }
            let children: Vec<_> = section["rows"].as_array().into_iter().flatten().map(|spec| c::setting_row(s,t,spec)).collect();
            if !children.is_empty() { page = page.child(c::section(title,children,t)); }
        }
        return page;
    }
    page.child(match path {
        "providers" => providers(s,t),
        "keybindings" => keybindings(s,t),
        "projects" => projects(s,t),
        "archived" => archived(s,t),
        "about" => about(t),
        _ => unavailable(path,t),
    })
}

fn themes(s: &App,t: &T) -> El {
    let mut modes = row().gap(4.0).padding(4.0).radius(8.0).fill(t.c.muted);
    for (v,label) in [("system","System"),("light","Light"),("dark","Dark")] {
        let selected = match s.appearance {crate::Appearance::System=>v=="system",crate::Appearance::Light=>v=="light",crate::Appearance::Dark=>v=="dark"};
        modes = modes.child(c::button(label,t).fill(if selected{t.c.background}else{t.c.muted}).on_press(move |s: &mut App| super::update(s,"appearance",json!(v))));
    }
    col().w_full().gap(16.0).child(row().w_full().center_y().child(text("Color scheme").font(theme::SM_MEDIUM).color(t.c.foreground).grow(1.0)).child(modes))
        .child(text("Themes").font(theme::weight(theme::TEXT_BASE,theme::SEMIBOLD)).color(t.c.foreground))
        .child(row().gap(12.0).child(theme_preview("Default",t)).child(theme_preview("Light",&theme::theme(false))).child(theme_preview("Dark",&theme::theme(true))))
}
fn theme_preview(label: &str,t: &T) -> El {
    col().w(160.0).gap(8.0).child(row().h(88.0).w_full().radius(8.0).border(1.0,t.c.border)
        .child(el().w(36.0).h_full().fill(t.sb.sidebar)).child(col().grow(1.0).h_full().padding(12.0).gap(8.0).fill(t.c.background)
        .child(el().h(6.0).w(64.0).radius(3.0).fill(t.c.muted)).child(el().h(24.0).w_full().radius(4.0).fill(t.c.card))))
        .child(text(label.to_owned()).font(theme::TEXT_XS).color(t.c.foreground))
}

fn providers(s: &App,t: &T) -> El {
    let mut content = col().w_full().gap(16.0)
        .child(row().w_full().center_y().child(text("Providers").font(theme::weight(theme::TEXT_BASE,theme::SEMIBOLD)).color(t.c.foreground).grow(1.0))
        .child(c::button("Refresh",t).on_press(|s: &mut App| request(s,"server.refreshProviders",json!({"fresh":true,"reloadSecrets":true})))));
    if let Some(providers) = s.state().config.as_ref().and_then(|c|c["providers"].as_array()) {
        for provider in providers {
            let name = provider["displayName"].as_str().or(provider["driver"].as_str()).unwrap_or("Provider");
            let id = provider["driver"].as_str().unwrap_or(name).to_owned();
            let version = provider["version"].as_str().unwrap_or("Not installed");
            let status = provider["status"].as_str().unwrap_or("");
            let head = row().w_full().padding(16.0).gap(12.0).center_y()
                .child(rune::icon(&icons::BOT,20.0).color(t.c.foreground))
                .child(col().grow(1.0).gap(4.0).child(text(name.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
                    .child(text(format!("{version}  {status}")).font(theme::TEXT_XS).color(t.c.muted_foreground)))
                .child(c::button("Check for updates",t).on_press(move |s: &mut App| request(s,"server.updateProvider",json!({"provider":id}))));
            content = content.child(col().w_full().border(1.0,t.c.border).radius(12.0).fill(t.c.card).child(head));
        }
    }
    content
}

fn projects(s: &App,t: &T) -> El {
    let children = s.state().projects.iter().map(|project| row().w_full().padding(16.0).gap(12.0)
        .child(rune::icon(&icons::FOLDER,16.0).color(t.c.muted_foreground))
        .child(col().gap(4.0).child(text(crate::data::s(project,"name").to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text(crate::data::s(project,"workspaceRoot").to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground)))).collect();
    c::section("Projects",children,t)
}
fn archived(s: &App,t: &T) -> El {
    let children = s.state().archived.iter().map(|thread| {
        row().padding(16.0).w_full().child(text(crate::data::s(thread,"title").to_owned()).font(theme::TEXT_SM).color(t.c.foreground))
    }).collect();
    c::section("Archived threads",children,t)
}
fn about(t: &T) -> El {
    col().gap(32.0).w_full().child(c::section("App",vec![row().padding(16.0).w_full().center_y()
        .child(col().grow(1.0).gap(4.0).child(text("S5 Code").font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text("Current version of the application.").font(theme::TEXT_XS).color(t.c.muted_foreground)))],t))
        .child(c::section("Legal",vec![text("Privacy policy").padding(16.0).font(theme::TEXT_SM).color(t.c.foreground),text("Open source licenses").padding(16.0).font(theme::TEXT_SM).color(t.c.foreground)],t))
}
fn keybindings(s: &App,t: &T) -> El {
    let mut rows = Vec::new();
    if let Some(bindings) = s.state().config.as_ref().and_then(|c| c["keybindings"].as_array()) {
        for binding in bindings {
            let command = binding["command"].as_str().unwrap_or("");
            let keys = shortcut_label(&binding["shortcut"]);
            let owned = command.to_owned();
            rows.push(row().w_full().padding(12.0).center_y().child(text(command.to_owned()).font(theme::TEXT_SM).color(t.c.foreground).grow(1.0))
                .child(c::button(&keys,t).on_press(move |s: &mut App| s.ui.settings.keybinding_editor=Some(owned.clone()))));
        }
    }
    let mut body = col().w_full().gap(16.0).child(c::section("Keybindings",rows,t));
    if let Some(command) = &s.ui.settings.keybinding_editor {
        body = body.child(col().padding(24.0).gap(16.0).radius(12.0).border(1.0,t.c.border).fill(t.c.popover)
            .child(text(format!("Edit shortcut: {command}")).font(theme::SM_MEDIUM).color(t.c.foreground))
            .child(text(s.ui.settings.shortcut.as_deref().unwrap_or("Press a key combination").to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground))
            .child(c::button("Save",t).disabled(s.ui.settings.shortcut.is_none()).on_press(|s: &mut App| {
                if let (Some(command),Some(key))=(s.ui.settings.keybinding_editor.clone(),s.ui.settings.shortcut.clone()) {
                    request(s,"server.upsertKeybinding",json!({"command":command,"key":key}));
                    s.ui.settings.keybinding_editor=None;s.ui.settings.shortcut=None;
                }
            }))
            .child(c::button("Cancel",t).on_press(|s: &mut App| s.ui.settings.keybinding_editor=None)));
    }
    body
}
fn unavailable(path: &str,t: &T) -> El {
    text(format!("{} settings are not yet available in the native client.",super::label(path))).font(theme::TEXT_SM).color(t.c.muted_foreground)
}

fn shortcut_label(shortcut: &Value) -> String {
    let mut out=String::new();
    for (field,symbol) in [("ctrlKey","⌃"),("altKey","⌥"),("shiftKey","⇧"),("metaKey","⌘"),("modKey","⌘")] {
        if shortcut[field].as_bool()==Some(true) {out.push_str(symbol);}
    }
    out.push_str(&shortcut["key"].as_str().unwrap_or("").to_uppercase());out
}
