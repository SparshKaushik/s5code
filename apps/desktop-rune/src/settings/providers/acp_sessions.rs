//! Web `AcpSessionManagementSection`: an ACP agent's native sessions (list, import as threads,
//! delete), its sign-out, and its configurable model providers, per project.
use super::super::{controls as c, permissions};
use crate::kit;
use crate::theme::{self, AlphaMul, T};
use crate::App;
use rune::{El, TextEdit, col, el, row, text};
use serde_json::{Map, Value, json};
use std::collections::HashMap;

#[derive(Default)]
pub struct Ui {
    project: Option<String>,
    sessions: Vec<Value>,
    next_cursor: Option<String>,
    /// Each request in flight, by what it does.
    loading: Option<(u64, bool)>,
    importing: Option<(u64, Value)>,
    deleting: Option<(u64, String)>,
    logging_out: Option<u64>,
    providers: Vec<Value>,
    drafts: HashMap<String, Draft>,
    loading_providers: Option<(u64, Option<String>)>,
    /// A provider being saved or (with `true`) disabled.
    saving: Option<(u64, String, bool)>,
}

#[derive(Clone, Default)]
struct Draft {
    api_type: String,
    base_url: TextEdit,
    headers: TextEdit,
}

fn ui_mut(s: &mut App) -> &mut Ui {
    &mut s.ui.settings.providers_ui.sessions
}

fn projects(s: &App) -> Vec<(String, String)> {
    s.state()
        .projects
        .iter()
        .filter_map(|p| Some((p["id"].as_str()?.to_owned(), p["title"].as_str().unwrap_or("").to_owned())))
        .collect()
}

fn project(s: &App) -> Option<String> {
    let ui = &s.ui.settings.providers_ui.sessions;
    ui.project.clone().or_else(|| projects(s).into_iter().next().map(|(id, _)| id))
}

/// `parseProviderHeaders`: blank is no headers; anything else must be a JSON object of strings.
pub fn parse_headers(text: &str) -> Result<Option<Map<String, Value>>, String> {
    if text.trim().is_empty() {
        return Ok(None);
    }
    let parsed: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
    match parsed {
        Value::Object(map) if map.values().all(Value::is_string) => Ok(Some(map)),
        _ => Err("Headers must be a JSON object with string values.".into()),
    }
}

fn draft_of(provider: &Value) -> Draft {
    let api_type = provider["current"]["apiType"].as_str().or(provider["supported"][0].as_str()).unwrap_or("").to_owned();
    Draft { api_type, base_url: TextEdit::new(provider["current"]["baseUrl"].as_str().unwrap_or("")), headers: TextEdit::new("").secure() }
}

/// The project select reads and writes through `provider::{id}::sessions.project`.
pub fn value(s: &App) -> Value {
    project(s).map_or(Value::Null, Value::String)
}

pub fn set(s: &mut App, value: &Value) {
    let Some(id) = value.as_str() else { return };
    let ui = ui_mut(s);
    if ui.loading.is_some() || ui.importing.is_some() || ui.deleting.is_some() || ui.loading_providers.is_some() || ui.saving.is_some() {
        return;
    }
    *ui = Ui { project: Some(id.to_owned()), logging_out: ui.logging_out, ..Ui::default() };
}

fn load_sessions(s: &mut App, instance: &str, cursor: Option<String>) {
    let Some(project) = project(s) else { return };
    if ui_mut(s).loading.is_some() {
        return;
    }
    let mut input = json!({"instanceId": instance, "projectId": project});
    if let Some(cursor) = &cursor {
        input["cursor"] = json!(cursor);
    }
    if let Some(id) = super::send(s, "server.listAcpRegistrySessions", input) {
        ui_mut(s).loading = Some((id, cursor.is_some()));
    }
}

fn load_providers(s: &mut App, instance: &str, reset: Option<String>) {
    let Some(project) = project(s) else { return };
    if ui_mut(s).loading_providers.is_some() {
        return;
    }
    if let Some(id) = super::send(s, "server.listAcpRegistryProviders", json!({"instanceId": instance, "projectId": project})) {
        ui_mut(s).loading_providers = Some((id, reset));
    }
}

fn save_provider(s: &mut App, instance: &str, provider: &str) {
    let Some(project) = project(s) else { return };
    let busy = ui_mut(s).saving.is_some() || ui_mut(s).loading_providers.is_some();
    let Some(draft) = ui_mut(s).drafts.get(provider).cloned() else { return };
    let base_url = draft.base_url.text.trim().to_owned();
    if busy || draft.api_type.is_empty() || base_url.is_empty() || !permissions::can(s, "providers:manage") {
        return;
    }
    let headers = match parse_headers(&draft.headers.text) {
        Ok(headers) => headers,
        Err(error) => return crate::shell::palette::toast_error_brief(s, "Invalid provider headers", Some(&error)),
    };
    let mut input = json!({"instanceId": instance, "projectId": project, "providerId": provider, "apiType": draft.api_type, "baseUrl": base_url});
    if let Some(headers) = headers {
        input["headers"] = Value::Object(headers);
    }
    if let Some(id) = super::send(s, "server.setAcpRegistryProvider", input) {
        ui_mut(s).saving = Some((id, provider.to_owned(), false));
    }
}

fn report(s: &mut App, title: &str, error: &Value) {
    crate::shell::palette::toast_error_brief(s, title, Some(&super::failure(error, "The ACP operation failed.")));
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let instance = super::selected_id(s).unwrap_or_default();
    if let Some((id, append)) = ui_mut(s).loading
        && let Some(reply) = super::take(s, id)
    {
        ui_mut(s).loading = None;
        match reply {
            Ok(result) => {
                let ui = ui_mut(s);
                let next = result["sessions"].as_array().cloned().unwrap_or_default();
                if append { ui.sessions.extend(next) } else { ui.sessions = next }
                ui.next_cursor = result["nextCursor"].as_str().map(str::to_owned);
            }
            Err(error) => report(s, "Could not list ACP sessions", &error),
        }
        changed = true;
    }
    if let Some((id, session)) = ui_mut(s).importing.clone()
        && let Some(reply) = super::take(s, id)
    {
        ui_mut(s).importing = None;
        match reply {
            Ok(result) => {
                for candidate in ui_mut(s).sessions.iter_mut().filter(|c| c["sessionId"] == session["sessionId"]) {
                    candidate["importedThreadId"] = result["threadId"].clone();
                }
                let title = if result["imported"] == true { "ACP session imported" } else { "ACP session already imported" };
                crate::shell::palette::toast(s, title, None);
            }
            Err(error) => report(s, "Could not import ACP session", &error),
        }
        changed = true;
    }
    if let Some((id, session)) = ui_mut(s).deleting.clone()
        && let Some(reply) = super::take(s, id)
    {
        ui_mut(s).deleting = None;
        match reply {
            Ok(_) => {
                ui_mut(s).sessions.retain(|c| c["sessionId"] != session.as_str());
                crate::shell::palette::toast(s, "ACP session deleted", None);
            }
            Err(error) => report(s, "Could not delete ACP session", &error),
        }
        changed = true;
    }
    if let Some(id) = ui_mut(s).logging_out
        && let Some(reply) = super::take(s, id)
    {
        ui_mut(s).logging_out = None;
        match reply {
            Ok(_) => {
                let ui = ui_mut(s);
                ui.sessions.clear();
                ui.next_cursor = None;
                crate::shell::palette::toast(s, "Logged out of ACP agent", None);
            }
            Err(error) => report(s, "Could not log out of ACP agent", &error),
        }
        changed = true;
    }
    if let Some((id, reset)) = ui_mut(s).loading_providers.clone()
        && let Some(reply) = super::take(s, id)
    {
        ui_mut(s).loading_providers = None;
        match reply {
            Ok(result) => {
                let ui = ui_mut(s);
                ui.providers = result["providers"].as_array().cloned().unwrap_or_default();
                // A refresh after one provider's save keeps the other providers' drafts.
                let mut drafts = HashMap::new();
                for p in &ui.providers {
                    let key = p["providerId"].as_str().unwrap_or("").to_owned();
                    let kept = ui.drafts.get(&key).filter(|_| reset.as_ref().is_some_and(|r| *r != key)).cloned();
                    drafts.insert(key, kept.unwrap_or_else(|| draft_of(p)));
                }
                ui.drafts = drafts;
            }
            Err(error) => report(s, "Could not list ACP providers", &error),
        }
        changed = true;
    }
    if let Some((id, provider, disable)) = ui_mut(s).saving.clone()
        && let Some(reply) = super::take(s, id)
    {
        ui_mut(s).saving = None;
        match reply {
            Ok(_) => {
                crate::shell::palette::toast(s, if disable { "ACP provider disabled" } else { "ACP provider configured" }, None);
                load_providers(s, &instance, Some(provider));
            }
            Err(error) => report(s, if disable { "Could not disable ACP provider" } else { "Could not configure ACP provider" }, &error),
        }
        changed = true;
    }
    changed
}

pub fn section(s: &App, t: &T, id: &str, p: &Value, read_only: bool) -> Option<El> {
    let native = &p["nativeSessions"];
    let can_list = native["canList"] == true;
    let can_import = native["canLoad"] == true || native["canResume"] == true;
    let can_delete = native["canDelete"] == true;
    let can_logout = p["auth"]["canLogout"] == true && p["setup"]["canAuthenticate"] != true;
    let can_configure = p["configurableProviders"] == true;
    if !can_list && !can_logout && !can_configure {
        return None;
    }
    let can_operate = permissions::can(s, "orchestration:operate");
    let can_read = permissions::can(s, "orchestration:read");
    let ui = &s.ui.settings.providers_ui.sessions;
    let projects = projects(s);
    let chosen = project(s);
    let busy = ui.loading.is_some() || ui.importing.is_some() || ui.deleting.is_some() || ui.loading_providers.is_some() || ui.saving.is_some();
    let select_key = super::field_key(id, "sessions.project");
    let options: Vec<(Value, String)> = projects.iter().map(|(pid, title)| (json!(pid), title.clone())).collect();
    let project_select = |label: &str| {
        let select = c::select(s, t, &select_key, label, &options, 160.0);
        if !can_read || busy { select.inert().opacity(0.5) } else { select }
    };

    let mut controls = row().gap(8.0).center_y().flex_wrap();
    if can_logout {
        let owned = id.to_owned();
        controls = controls.child(
            kit::Button::new(if ui.logging_out.is_some() { "Logging out" } else { "Log out" })
                .variant("outline")
                .size("sm")
                .disabled(read_only || ui.logging_out.is_some())
                .on_press(move |s: &mut App| {
                    if ui_mut(s).logging_out.is_some() || !permissions::can(s, "providers:manage") {
                        return;
                    }
                    if let Some(reply) = super::send(s, "server.logoutAcpRegistry", json!({"instanceId": owned})) {
                        ui_mut(s).logging_out = Some(reply);
                    }
                })
                .build(t),
        );
    }
    if can_list && !projects.is_empty() {
        let owned = id.to_owned();
        let label = if ui.loading.is_some() { "Loading" } else if ui.sessions.is_empty() { "List sessions" } else { "Refresh" };
        controls = controls.child(project_select("Project for ACP sessions")).child(
            kit::Button::new(label)
                .variant("outline")
                .size("sm")
                .disabled(!can_read || ui.loading.is_some() || chosen.is_none())
                .on_press(move |s: &mut App| load_sessions(s, &owned, None))
                .build(t),
        );
    }
    let mut copy = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(text("Native sessions").font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text("Resume agent-owned conversations as T3 threads.").font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap());
    if can_list && projects.is_empty() {
        copy = copy.child(text("Add a project before importing sessions.").font(theme::TEXT_XS).color(t.c.muted_foreground).wrap().pt(2.0));
    }
    let mut native_row = col().w_full().px(16.0).py(12.0).gap(12.0).child(row().w_full().gap(32.0).center_y().child(copy).child(controls));
    if can_list && !ui.sessions.is_empty() {
        let mut list = col().w_full().border(1.0, t.c.border.alpha_mul(0.6));
        for (index, session) in ui.sessions.iter().enumerate() {
            let session_id = session["sessionId"].as_str().unwrap_or("").to_owned();
            let title = session["title"].as_str().map_or_else(|| session_id.clone(), str::to_owned);
            let imported = !session["importedThreadId"].is_null();
            let importing = ui.importing.as_ref().is_some_and(|(_, c)| c["sessionId"] == session["sessionId"]);
            let (import_id, import_session) = (id.to_owned(), session.clone());
            let mut actions = row().gap(4.0).center_y().child(
                kit::Button::new(if imported { "Imported" } else if importing { "Importing" } else { "Import" })
                    .variant("ghost-muted")
                    .size("xs")
                    .disabled(!can_operate || !can_import || imported || ui.importing.is_some())
                    .on_press(move |s: &mut App| {
                        let Some(project) = project(s) else { return };
                        if ui_mut(s).importing.is_some() || !permissions::can(s, "orchestration:operate") {
                            return;
                        }
                        let input = json!({
                            "instanceId": import_id, "projectId": project, "sessionId": import_session["sessionId"],
                            "title": import_session["title"], "updatedAt": import_session["updatedAt"],
                        });
                        if let Some(reply) = super::send(s, "server.importAcpRegistrySession", input) {
                            ui_mut(s).importing = Some((reply, import_session.clone()));
                        }
                    })
                    .build(t),
            );
            if can_delete {
                let deleting = ui.deleting.as_ref().is_some_and(|(_, d)| *d == session_id);
                let (delete_id, delete_session, delete_title) = (id.to_owned(), session_id.clone(), title.clone());
                actions = actions.child(
                    kit::Button::new(if deleting { "Deleting" } else { "Delete" })
                        .variant("ghost-muted")
                        .size("xs")
                        .disabled(!can_operate || imported || ui.deleting.is_some())
                        .on_press(move |s: &mut App| {
                            let (instance, session) = (delete_id.clone(), delete_session.clone());
                            crate::shell::confirm::request(s, format!("Permanently delete native ACP session \"{delete_title}\"?"), true, move |s: &mut App| {
                                let Some(project) = project(s) else { return };
                                if ui_mut(s).deleting.is_some() || !permissions::can(s, "orchestration:operate") {
                                    return;
                                }
                                let input = json!({"instanceId": instance, "projectId": project, "sessionId": session});
                                if let Some(reply) = super::send(s, "server.deleteAcpRegistrySession", input) {
                                    ui_mut(s).deleting = Some((reply, session.clone()));
                                }
                            });
                        })
                        .build(t),
                );
            }
            if index > 0 {
                list = list.child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6)));
            }
            list = list.child(
                row()
                    .key(format!("acp-session-{session_id}"))
                    .w_full()
                    .py(8.0)
                    .gap(12.0)
                    .center_y()
                    .child(
                        col()
                            .grow(1.0)
                            .min_w(0.0)
                            .child(text(title).font(theme::TEXT_XS).color(t.c.foreground).truncate())
                            .child(text(session_id.clone()).font(theme::mono(theme::TEXT_2XS)).color(t.c.muted_foreground).truncate()),
                    )
                    .child(actions),
            );
        }
        native_row = native_row.child(list);
    }
    if can_list && let Some(cursor) = ui.next_cursor.clone() {
        let owned = id.to_owned();
        native_row = native_row.child(
            kit::Button::new(if ui.loading.is_some() { "Loading" } else { "Load more" })
                .variant("ghost-muted")
                .size("xs")
                .disabled(!can_read || ui.loading.is_some())
                .on_press(move |s: &mut App| load_sessions(s, &owned, Some(cursor.clone())))
                .build(t)
                .self_start(),
        );
    }
    let mut out = col().w_full().gap(12.0).child(native_row);
    if can_configure {
        let owned = id.to_owned();
        let label = if ui.loading_providers.is_some() { "Loading" } else if ui.providers.is_empty() { "List providers" } else { "Refresh" };
        let mut block = col().w_full().px(16.0).py(12.0).gap(12.0).child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .child(text("Agent providers").font(theme::XS_MEDIUM).color(t.c.foreground))
                        .child(text("Configure non-secret routing. Headers are write-only.").font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()),
                )
                .child(
                    kit::Button::new(label)
                        .variant("outline")
                        .size("xs")
                        .disabled(!can_read || ui.loading_providers.is_some() || ui.saving.is_some() || chosen.is_none())
                        .on_press(move |s: &mut App| load_providers(s, &owned, None))
                        .build(t),
                ),
        );
        if !can_list && projects.len() > 1 {
            block = block.child(project_select("Project for ACP providers"));
        }
        for provider in &ui.providers {
            block = block.child(provider_row(s, t, id, provider, read_only));
        }
        out = out.child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6))).child(block);
    }
    Some(out)
}

fn provider_row(s: &App, t: &T, id: &str, provider: &Value, read_only: bool) -> El {
    let ui = &s.ui.settings.providers_ui.sessions;
    let key = provider["providerId"].as_str().unwrap_or("").to_owned();
    let draft = ui.drafts.get(&key).cloned().unwrap_or_else(|| draft_of(provider));
    let saving = ui.saving.is_some();
    let mut status = if provider["current"].is_null() { "Disabled".to_owned() } else { "Configured".to_owned() };
    if provider["required"] == true {
        status += " · Required";
    }
    let (save_id, save_key) = (id.to_owned(), key.clone());
    let mut actions = row().gap(4.0).center_y().child(
        kit::Button::new(if ui.saving.as_ref().is_some_and(|(_, p, d)| *p == key && !d) { "Saving" } else { "Save" })
            .variant("outline")
            .size("xs")
            .disabled(read_only || saving || ui.loading_providers.is_some() || draft.api_type.is_empty() || draft.base_url.text.trim().is_empty())
            .on_press(move |s: &mut App| save_provider(s, &save_id, &save_key))
            .build(t),
    );
    if provider["required"] != true && !provider["current"].is_null() {
        let (disable_id, disable_key) = (id.to_owned(), key.clone());
        actions = actions.child(
            kit::Button::new("Disable")
                .variant("ghost-muted")
                .size("xs")
                .disabled(read_only || saving || ui.loading_providers.is_some())
                .on_press(move |s: &mut App| {
                    let (instance, provider) = (disable_id.clone(), disable_key.clone());
                    crate::shell::confirm::request(s, format!("Disable ACP provider \"{provider}\"?"), true, move |s: &mut App| {
                        let Some(project) = project(s) else { return };
                        if ui_mut(s).saving.is_some() || ui_mut(s).loading_providers.is_some() || !permissions::can(s, "providers:manage") {
                            return;
                        }
                        let input = json!({"instanceId": instance, "projectId": project, "providerId": provider});
                        if let Some(reply) = super::send(s, "server.disableAcpRegistryProvider", input) {
                            ui_mut(s).saving = Some((reply, provider.clone(), true));
                        }
                    });
                })
                .build(t),
        );
    }
    // Web's protocol select, as a row of choices: `supported` is short.
    let mut protocols = row().gap(4.0).center_y().role("radiogroup").label(format!("{key} protocol"));
    for protocol in provider["supported"].as_array().into_iter().flatten().filter_map(Value::as_str) {
        let (owned, choice, chosen) = (key.clone(), protocol.to_owned(), draft.api_type == protocol);
        protocols = protocols.child(
            kit::Button::new(protocol)
                .variant(if chosen { "secondary" } else { "ghost-muted" })
                .size("xs")
                .disabled(read_only || saving)
                .on_press(move |s: &mut App| {
                    let draft = ui_mut(s).drafts.entry(owned.clone()).or_default();
                    draft.api_type = choice.clone();
                })
                .build(t)
                .role("radio")
                .checked(chosen),
        );
    }
    let (url_key, headers_key) = (key.clone(), key.clone());
    let base_url = kit::Input::new(&draft.base_url)
        .size("sm")
        .placeholder("https://api.example.com")
        .disabled(read_only || saving)
        .on_edit(move |s: &mut App| &mut ui_mut(s).drafts.entry(url_key.clone()).or_default().base_url)
        .build(t)
        .key(format!("acp-provider-{key}-url"))
        .single_line()
        .grow(1.0)
        .min_w(0.0)
        .label(format!("{key} base URL"));
    let headers = kit::Input::new(&draft.headers)
        .size("sm")
        .placeholder("Write-only headers JSON, e.g. {\"Authorization\":\"Bearer …\"}")
        .disabled(read_only || saving)
        .on_edit(move |s: &mut App| &mut ui_mut(s).drafts.entry(headers_key.clone()).or_insert_with(|| Draft { headers: TextEdit::new("").secure(), ..Draft::default() }).headers)
        .build(t)
        .key(format!("acp-provider-{key}-headers"))
        .single_line()
        .secure()
        .w_full()
        .label(format!("{key} write-only headers JSON"));
    col()
        .key(format!("acp-provider-{key}"))
        .w_full()
        .py(12.0)
        .gap(8.0)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(
                    col()
                        .grow(1.0)
                        .min_w(0.0)
                        .child(text(key.clone()).font(theme::XS_MEDIUM).color(t.c.foreground).truncate())
                        .child(text(status).font(theme::TEXT_2XS).color(t.c.muted_foreground)),
                )
                .child(actions),
        )
        .child(row().w_full().gap(8.0).center_y().child(protocols).child(base_url))
        .child(headers)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_headers_must_be_an_object_of_strings() {
        assert_eq!(parse_headers("  "), Ok(None));
        let parsed = parse_headers(r#"{"Authorization": "Bearer x"}"#).unwrap().unwrap();
        assert_eq!(parsed["Authorization"], "Bearer x");
        assert_eq!(parse_headers(r#"{"n": 1}"#), Err("Headers must be a JSON object with string values.".into()));
        assert_eq!(parse_headers(r#"["a"]"#), Err("Headers must be a JSON object with string values.".into()));
        assert!(parse_headers("{").is_err());
    }
}
