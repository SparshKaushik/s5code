//! The device host editors: web `DeviceHostEditor` (a machine over SSH) and `EasDeviceHostEditor`.
//! Their text fields live in `devices::Fields`; this module keeps the rest of the draft.

use super::devices::{environment_label, error_text, save_settings, settings_list, update_device_hosts, update_eas_hosts};
use crate::kit;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, TextEdit, col, el, icon, row, text};
use serde_json::{Value, json};

const FROM_THREAD_PROJECT: &str = "From the thread's project";

pub struct Editor {
    kind: Kind,
    /// The settings write in flight; the editor closes once it lands without an error.
    saving: Option<u64>,
    error: Option<String>,
}

enum Kind {
    Ssh(Ssh),
    Eas(Eas),
}

struct Ssh {
    original: Option<Value>,
    id: String,
    options: bool,
    /// The last connection check and the destination it checked.
    check: Option<(String, Check)>,
    show_error: bool,
}

enum Check {
    Pending(u64),
    Local,
    Connected(Vec<Value>),
    Failed(String),
}

struct Eas {
    host: Value,
    is_new: bool,
    /// The Clear button was pressed; typing a new token undoes it.
    cleared: bool,
    app_id: Option<String>,
    app_menu: bool,
    account: Account,
}

enum Account {
    Idle,
    Checking(u64),
    SignedIn(Value),
    Failed(String),
}

#[derive(Clone, Copy, PartialEq, Debug)]
enum Token {
    Saved,
    New,
    Cleared,
}

/// A saved token arrives redacted; only a replacement or the Clear button changes it.
fn token_state(text: &str, had_token: bool, cleared: bool) -> Token {
    if !text.is_empty() {
        Token::New
    } else if had_token && !cleared {
        Token::Saved
    } else {
        Token::Cleared
    }
}

fn has_token(host: &Value) -> bool {
    match &host["accessToken"] {
        Value::String(token) => !token.is_empty(),
        Value::Object(_) => true,
        _ => false,
    }
}

/// `SshDeviceHostConfig` from the draft, or `None` where the schema would reject it. An empty
/// name falls back to the target, as web parses it; Save still asks for a name.
fn ssh_host(id: &str, label: &str, target: &str, identity: &str, port: &str) -> Option<Value> {
    let valid_target = !target.is_empty() && !target.starts_with('-') && !target.chars().any(char::is_whitespace);
    if !valid_target {
        return None;
    }
    let label = match label.trim() {
        "" => target,
        label => label,
    };
    let mut host = json!({"kind": "ssh", "id": id, "label": label, "target": target});
    if !identity.trim().is_empty() {
        host["identityFile"] = json!(identity.trim());
    }
    if !port.trim().is_empty() {
        let port = port.trim().parse::<u16>().ok().filter(|port| *port >= 1)?;
        host["port"] = json!(port);
    }
    Some(host)
}

/// `deviceHostConnectionKey`: a check answers for one destination.
fn connection_key(target: &str, port: &str, identity: &str) -> String {
    format!("{}\u{0}{}\u{0}{}", target.trim(), port.trim(), identity.trim())
}

fn number(text: &str) -> i64 {
    text.trim().parse().unwrap_or(0)
}

/// Why Save is off for an EAS draft, or `None` when the draft is valid.
fn eas_problem(label: &str, sessions: i64, session_minutes: i64, idle_minutes: i64) -> Option<&'static str> {
    let valid = !label.trim().is_empty()
        && (1..=20).contains(&sessions)
        && (1..=115).contains(&session_minutes)
        && (1..=60).contains(&idle_minutes)
        && idle_minutes < session_minutes;
    if valid {
        None
    } else if session_minutes > 0 && idle_minutes >= session_minutes {
        Some("Idle minutes must be smaller than session minutes.")
    } else {
        Some("Give the host a label and limits within the ranges shown.")
    }
}

/// The EAS host Save writes: the original with the draft's fields over it.
fn eas_host(original: &Value, label: &str, limits: [i64; 3], app_id: Option<&str>, token: Token, typed: &str) -> Value {
    let mut host = original.clone();
    host["label"] = json!(label.trim());
    host["maxConcurrentSessions"] = json!(limits[0]);
    host["maxSessionMinutes"] = json!(limits[1]);
    host["idleMinutes"] = json!(limits[2]);
    match app_id {
        Some(id) => host["appId"] = json!(id),
        None => {
            if let Some(fields) = host.as_object_mut() {
                fields.remove("appId");
            }
        }
    }
    host["accessToken"] = match token {
        Token::Saved => original["accessToken"].clone(),
        Token::New => json!(typed),
        Token::Cleared => json!(""),
    };
    host
}

fn str_of(host: &Value, key: &str) -> String {
    host[key].as_str().unwrap_or("").to_owned()
}

fn ssh_mut(s: &mut App) -> Option<&mut Ssh> {
    match s.ui.settings.devices.editor.as_mut() {
        Some(Editor { kind: Kind::Ssh(ssh), .. }) => Some(ssh),
        _ => None,
    }
}

fn eas_mut(s: &mut App) -> Option<&mut Eas> {
    match s.ui.settings.devices.editor.as_mut() {
        Some(Editor { kind: Kind::Eas(eas), .. }) => Some(eas),
        _ => None,
    }
}

pub fn saving(s: &App) -> bool {
    s.ui.settings.devices.editor.as_ref().is_some_and(|editor| editor.saving.is_some())
}

pub fn open_ssh(s: &mut App, original: Option<Value>) {
    let host = original.clone().unwrap_or_else(|| json!({"kind": "ssh", "id": s5_net::uuid(), "label": "", "target": ""}));
    let fields = &mut s.ui.settings.devices.fields;
    fields.label = TextEdit::new(str_of(&host, "label"));
    fields.label.focused = true;
    fields.target = TextEdit::new(str_of(&host, "target"));
    fields.identity = TextEdit::new(str_of(&host, "identityFile"));
    fields.port = TextEdit::new(host["port"].as_i64().map(|port| port.to_string()).unwrap_or_default());
    let options = host.get("port").is_some() || host.get("identityFile").is_some();
    s.ui.settings.devices.editor = Some(Editor {
        kind: Kind::Ssh(Ssh { original, id: str_of(&host, "id"), options, check: None, show_error: false }),
        saving: None,
        error: None,
    });
}

pub fn open_eas(s: &mut App, saved: Option<Value>) {
    let is_new = saved.is_none();
    let host = saved.unwrap_or_else(|| {
        json!({
            "kind": "eas",
            "id": s5_net::uuid(),
            "label": "EAS Simulator",
            "accessToken": "",
            "maxSessionMinutes": 30,
            "idleMinutes": 10,
            "maxConcurrentSessions": 2,
        })
    });
    let fields = &mut s.ui.settings.devices.fields;
    fields.label = TextEdit::new(str_of(&host, "label"));
    fields.label.focused = true;
    fields.token = TextEdit::new("").secure();
    fields.sessions = TextEdit::new(host["maxConcurrentSessions"].as_i64().unwrap_or(2).to_string());
    fields.session_minutes = TextEdit::new(host["maxSessionMinutes"].as_i64().unwrap_or(30).to_string());
    fields.idle_minutes = TextEdit::new(host["idleMinutes"].as_i64().unwrap_or(10).to_string());
    // Checking on open names the saved project and shows a rejected credential early.
    let input = match (has_token(&host), is_new) {
        (true, true) => json!({}),
        (true, false) => json!({"hostId": host["id"]}),
        (false, _) => json!({"accessToken": ""}),
    };
    let account = match s.src.net() {
        Some(net) => Account::Checking(net.request("device.easAccount", input)),
        None => Account::Idle,
    };
    let app_id = host["appId"].as_str().map(str::to_owned);
    s.ui.settings.devices.editor = Some(Editor {
        kind: Kind::Eas(Eas { host, is_new, cleared: false, app_id, app_menu: false, account }),
        saving: None,
        error: None,
    });
}

/// Opens the editor for a source row key (`ssh:{id}` or `eas:{id}`).
pub fn edit(s: &mut App, key: &str) {
    let Some((kind, id)) = key.split_once(':') else { return };
    let field = match kind {
        "ssh" => "deviceHosts",
        "eas" => "easDeviceHosts",
        _ => return,
    };
    let Some(host) = settings_list(s, field).into_iter().find(|host| host["id"] == id) else { return };
    if kind == "ssh" { open_ssh(s, Some(host)) } else { open_eas(s, Some(host)) }
}

fn close(s: &mut App) {
    if !saving(s) {
        s.ui.settings.devices.editor = None;
    }
}

pub fn escape(s: &mut App) -> bool {
    if let Some(eas) = eas_mut(s)
        && std::mem::take(&mut eas.app_menu)
    {
        return true;
    }
    if s.ui.settings.devices.editor.is_none() {
        return false;
    }
    close(s);
    true
}

pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    let waiting = match &s.ui.settings.devices.editor {
        Some(Editor { kind: Kind::Ssh(Ssh { check: Some((_, Check::Pending(id))), .. }), .. }) => Some(*id),
        Some(Editor { kind: Kind::Eas(Eas { account: Account::Checking(id), .. }), .. }) => Some(*id),
        _ => None,
    };
    if let Some(id) = waiting
        && let Some(reply) = s.src.net().and_then(|net| net.take_reply(id))
    {
        if let Some(ssh) = ssh_mut(s)
            && let Some((_, check)) = ssh.check.as_mut()
        {
            *check = match reply {
                Ok(summary) if summary["kind"] == "local" => Check::Local,
                Ok(summary) => Check::Connected(summary["platforms"].as_array().cloned().unwrap_or_default()),
                Err(cause) => Check::Failed(error_text(&cause)),
            };
        } else if let Some(eas) = eas_mut(s) {
            eas.account = match reply {
                Ok(account) => Account::SignedIn(account),
                Err(cause) => Account::Failed(error_text(&cause)),
            };
        }
        changed = true;
    }
    let saving = s.ui.settings.devices.editor.as_ref().and_then(|editor| editor.saving);
    if let Some(id) = saving
        && !s.ui.settings.pending.contains_key(&id)
    {
        match s.ui.settings.error.clone() {
            None => s.ui.settings.devices.editor = None,
            Some(error) => {
                if let Some(editor) = s.ui.settings.devices.editor.as_mut() {
                    editor.saving = None;
                    editor.error = Some(error);
                }
            }
        }
        changed = true;
    }
    changed
}

fn commit(s: &mut App, title: &str, patch: Result<Value, String>) {
    match patch {
        Ok(patch) => {
            let Some(id) = save_settings(s, patch) else {
                s.ui.settings.devices.editor = None;
                return;
            };
            if let Some(editor) = s.ui.settings.devices.editor.as_mut() {
                editor.saving = Some(id);
                editor.error = None;
            }
        }
        Err(message) => {
            if let Some(editor) = s.ui.settings.devices.editor.as_mut() {
                editor.error = Some(format!("{title}. {message}"));
            }
        }
    }
}

fn save(s: &mut App) {
    let Some(editor) = &s.ui.settings.devices.editor else { return };
    if editor.saving.is_some() {
        return;
    }
    let ssh = matches!(editor.kind, Kind::Ssh(_));
    if ssh { save_ssh(s) } else { save_eas(s) }
}

fn save_ssh(s: &mut App) {
    let Some(Editor { kind: Kind::Ssh(ssh), .. }) = &s.ui.settings.devices.editor else { return };
    let f = &s.ui.settings.devices.fields;
    if f.label.text.trim().is_empty() || matches!(ssh.check, Some((_, Check::Pending(_)))) {
        return;
    }
    let Some(host) = ssh_host(&ssh.id, &f.label.text, &f.target.text, &f.identity.text, &f.port.text) else { return };
    let original = ssh.original.clone().unwrap_or_else(|| host.clone());
    let patch = update_device_hosts(&settings_list(s, "deviceHosts"), &host, false, &original)
        .map(|hosts| json!({"deviceHosts": hosts}));
    commit(s, "Device host not saved everywhere", patch);
}

fn eas_draft(s: &App, eas: &Eas) -> (Value, Option<&'static str>, Token) {
    let f = &s.ui.settings.devices.fields;
    let limits = [number(&f.sessions.text), number(&f.session_minutes.text), number(&f.idle_minutes.text)];
    let token = token_state(&f.token.text, has_token(&eas.host), eas.cleared);
    let host = eas_host(&eas.host, &f.label.text, limits, eas.app_id.as_deref(), token, &f.token.text);
    (host, eas_problem(&f.label.text, limits[0], limits[1], limits[2]), token)
}

fn save_eas(s: &mut App) {
    let Some(Editor { kind: Kind::Eas(eas), .. }) = &s.ui.settings.devices.editor else { return };
    let (host, problem, token) = eas_draft(s, eas);
    if problem.is_some() {
        return;
    }
    let keep_token = !eas.is_new && token == Token::Saved;
    let hosts = update_eas_hosts(&settings_list(s, "easDeviceHosts"), &host, false, keep_token);
    commit(s, "EAS host not saved everywhere", Ok(json!({"easDeviceHosts": hosts})));
}

fn test_connection(s: &mut App) {
    let Some(Editor { kind: Kind::Ssh(ssh), .. }) = &s.ui.settings.devices.editor else { return };
    let f = &s.ui.settings.devices.fields;
    let Some(host) = ssh_host(&ssh.id, &f.label.text, &f.target.text, &f.identity.text, &f.port.text) else { return };
    let key = connection_key(&f.target.text, &f.port.text, &f.identity.text);
    let Some(net) = s.src.net() else { return };
    let id = net.request("device.testHost", host);
    if let Some(ssh) = ssh_mut(s) {
        ssh.check = Some((key, Check::Pending(id)));
        ssh.show_error = false;
    }
}

fn check_account(s: &mut App) {
    let Some(Editor { kind: Kind::Eas(eas), .. }) = &s.ui.settings.devices.editor else { return };
    let f = &s.ui.settings.devices.fields;
    // A cleared token is sent empty so the check cannot fall back to the saved one.
    let input = match token_state(&f.token.text, has_token(&eas.host), eas.cleared) {
        Token::Saved if !eas.is_new => json!({"hostId": eas.host["id"]}),
        Token::Saved => json!({}),
        Token::New => json!({"accessToken": f.token.text}),
        Token::Cleared => json!({"accessToken": ""}),
    };
    let Some(net) = s.src.net() else { return };
    let id = net.request("device.easAccount", input);
    if let Some(eas) = eas_mut(s) {
        eas.account = Account::Checking(id);
    }
}

fn token_edited(s: &mut App) {
    if let Some(eas) = eas_mut(s) {
        eas.cleared = false;
        eas.account = Account::Idle;
    }
}

fn clear_token(s: &mut App) {
    s.ui.settings.devices.fields.token = TextEdit::new("").secure();
    if let Some(eas) = eas_mut(s) {
        eas.cleared = true;
        eas.account = Account::Idle;
    }
}

fn field(t: &T, label: &str, control: El) -> El {
    col().w_full().min_w(0.0).gap(6.0).child(text(label.to_owned()).font(theme::TEXT_SM).color(t.c.foreground)).child(control)
}

fn note(t: &T, copy: impl Into<String>) -> El {
    text(copy.into()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()
}

fn boxed(t: &T) -> El {
    col().w_full().radius(8.0).border(1.0, t.c.border.alpha_mul(0.6))
}

fn footer(t: &T, busy: bool, can_save: bool) -> El {
    row()
        .w_full()
        .gap(8.0)
        .justify_end()
        .child(kit::Button::new("Cancel").variant("ghost").disabled(busy).on_press(close).build(t))
        .child(
            kit::Button::new(if busy { "Saving…" } else { "Save host" })
                .disabled(busy || !can_save)
                .on_press(save)
                .build(t),
        )
}

pub fn dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(editor) = &s.ui.settings.devices.editor else { return };
    match &editor.kind {
        Kind::Ssh(ssh) => ssh_dialog(s, t, editor, ssh, layers),
        Kind::Eas(eas) => eas_dialog(s, t, editor, eas, layers),
    }
}

fn ssh_dialog(s: &App, t: &T, editor: &Editor, ssh: &Ssh, layers: &mut Vec<El>) {
    let f = &s.ui.settings.devices.fields;
    let k = kit::theme(t);
    let busy = editor.saving.is_some();
    let host = ssh_host(&ssh.id, &f.label.text, &f.target.text, &f.identity.text, &f.port.text);
    let key = connection_key(&f.target.text, &f.port.text, &f.identity.text);
    let result = ssh.check.as_ref().filter(|(checked, _)| *checked == key).map(|(_, check)| check);
    let checking = matches!(result, Some(Check::Pending(_)));
    let summary = match result {
        Some(Check::Pending(_)) => "Checking environments…",
        Some(Check::Failed(_)) => "1 of 1 failed",
        Some(_) => "Connection checks passed",
        None => "Check access before saving",
    };
    let muted = t.c.muted_foreground;

    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(field(
            t,
            "Name",
            kit::Input::new(&f.label)
                .placeholder("Mac mini")
                .disabled(busy)
                .on_edit(|s: &mut App| &mut s.ui.settings.devices.fields.label)
                .on_submit(|s: &mut App, _: &mut rune::Tx| save(s))
                .build(t)
                .single_line(),
        ))
        .child(field(
            t,
            "SSH target",
            kit::Input::new(&f.target)
                .placeholder("user@host or SSH alias")
                .disabled(busy)
                .on_edit(|s: &mut App| &mut s.ui.settings.devices.fields.target)
                .on_submit(|s: &mut App, _: &mut rune::Tx| save(s))
                .build(t)
                .single_line(),
        ));

    let mut options = col().w_full().child(
        row()
            .gap(6.0)
            .center_y()
            .focusable(true)
            .role("button")
            .label("SSH options")
            .expanded(ssh.options)
            .cursor(rune::Cursor::Pointer)
            .on_press(|s: &mut App| {
                if let Some(ssh) = ssh_mut(s) {
                    ssh.options = !ssh.options;
                }
            })
            .child(icon(if ssh.options { &icons::CHEVRON_DOWN } else { &icons::CHEVRON_RIGHT }, 14.0).color(muted))
            .child(text("SSH options").font(theme::TEXT_SM).color(muted)),
    );
    if ssh.options {
        options = options
            .child(
                row()
                    .w_full()
                    .mt(12.0)
                    .gap(12.0)
                    .items_start()
                    .child(
                        el().grow(1.0).min_w(0.0).child(field(
                            t,
                            "Identity file",
                            kit::Input::new(&f.identity)
                                .placeholder("SSH config default")
                                .disabled(busy)
                                .on_edit(|s: &mut App| &mut s.ui.settings.devices.fields.identity)
                                .build(t)
                                .single_line(),
                        )),
                    )
                    .child(
                        el().w(112.0).child(field(
                            t,
                            "Port",
                            kit::Input::new(&f.port)
                                .placeholder("Default")
                                .disabled(busy)
                                .invalid(!f.port.text.trim().is_empty() && host.is_none())
                                .on_edit(|s: &mut App| &mut s.ui.settings.devices.fields.port)
                                .build(t)
                                .single_line(),
                        )),
                    ),
            )
            .child(note(t, "Optional. Resolved separately on each environment.").mt(8.0));
    }
    body = body.child(options);

    let mut checks = boxed(t).child(
        row()
            .w_full()
            .px(12.0)
            .py(10.0)
            .gap(12.0)
            .center_y()
            .justify_between()
            .child(text(summary).font(theme::TEXT_XS).color(muted).role("status").shrink())
            .child(
                kit::Button::new("Test connection")
                    .variant("outline")
                    .size("sm")
                    .disabled(busy || checking || host.is_none())
                    .on_press(test_connection)
                    .build(t),
            ),
    );
    if let Some(check) = result {
        let (glyph, label, color) = match check {
            Check::Pending(_) => (None, "Checking…", muted),
            Check::Local => (Some(&icons::MONITOR), "Already available locally", muted),
            Check::Failed(_) => (Some(&icons::X), "Failed", t.c.destructive),
            Check::Connected(_) => (Some(&icons::CHECK), "Connected", muted),
        };
        let status = row()
            .gap(6.0)
            .center_y()
            .children(checking.then(|| kit::Spinner::new().size("xs").tone("muted").build(&k)))
            .children(glyph.map(|glyph: &'static rune::Icon| icon(glyph, 12.0).color(color)))
            .child(text(label).font(theme::TEXT_XS).color(color));
        let mut item = col().w_full().px(12.0).py(10.0).child(
            row()
                .w_full()
                .gap(12.0)
                .center_y()
                .justify_between()
                .child(text(environment_label(s)).font(theme::XS_MEDIUM).color(t.c.foreground).truncate().shrink())
                .child(status),
        );
        match check {
            Check::Connected(platforms) => item = item.child(availability(t, platforms).mt(6.0)),
            Check::Failed(error) => {
                item = item.child(
                    text("Show error")
                        .font(theme::TEXT_XS)
                        .color(muted)
                        .mt(6.0)
                        .focusable(true)
                        .role("button")
                        .expanded(ssh.show_error)
                        .cursor(rune::Cursor::Pointer)
                        .on_press(|s: &mut App| {
                            if let Some(ssh) = ssh_mut(s) {
                                ssh.show_error = !ssh.show_error;
                            }
                        }),
                );
                if ssh.show_error {
                    item = item.child(note(t, error.clone()).mt(4.0));
                }
            }
            _ => {}
        }
        checks = checks.child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.6))).child(item);
    }
    body = body.child(checks);
    if let Some(error) = &editor.error {
        body = body.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap().role("alert"));
    }
    let can_save = !checking && host.is_some() && !f.label.text.trim().is_empty();
    kit::Dialog::new("device-host-editor", if ssh.original.is_none() { "Add device host" } else { "Edit device host" })
        .open(true)
        .description(format!("Connect from {}. Hosts on the same machine are skipped.", environment_label(s)))
        .on_close(close)
        .child(body)
        .child(footer(t, busy, can_save))
        .build(t, layers);
}

/// `DeviceHostAvailability`: each platform a reached host reports.
fn availability(t: &T, platforms: &[Value]) -> El {
    let muted = t.c.muted_foreground;
    row().w_full().flex_wrap().gap(16.0).children(platforms.iter().map(|platform| {
        let available = platform["available"] == true;
        let name = if platform["platform"] == "ios" { "iOS" } else { "Android" };
        let reason = platform["reason"].as_str().map(str::to_owned).unwrap_or_else(|| format!("{name} available"));
        row()
            .gap(4.0)
            .center_y()
            .focusable(true)
            .description(reason)
            .child(icon(if available { &icons::CHECK } else { &icons::MINUS }, 12.0).color(muted))
            .child(text(format!("{name} {}", if available { "available" } else { "unavailable" })).font(theme::TEXT_XS).color(muted))
    }))
}

/// The project choices: label and the `appId` it saves.
fn app_choices(eas: &Eas) -> Vec<(String, Option<String>)> {
    let apps: Vec<(String, String)> = match &eas.account {
        Account::SignedIn(account) => account["apps"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|app| Some((app["id"].as_str()?.to_owned(), app["fullName"].as_str()?.to_owned())))
            .collect(),
        _ => Vec::new(),
    };
    let mut choices = vec![(FROM_THREAD_PROJECT.to_owned(), None)];
    if let Some(id) = &eas.app_id
        && !apps.iter().any(|(app, _)| app == id)
    {
        choices.push((id.clone(), Some(id.clone())));
    }
    choices.extend(apps.into_iter().map(|(id, name)| (name, Some(id))));
    choices
}

fn credential_source(source: &str) -> &'static str {
    match source {
        "settings" => "the access token saved here",
        "env" => "EXPO_TOKEN on the server",
        _ => "the server's eas login",
    }
}

fn eas_dialog(s: &App, t: &T, editor: &Editor, eas: &Eas, layers: &mut Vec<El>) {
    let f = &s.ui.settings.devices.fields;
    let k = kit::theme(t);
    let busy = editor.saving.is_some();
    let muted = t.c.muted_foreground;
    let (_, problem, token) = eas_draft(s, eas);

    let mut token_row = row().w_full().gap(8.0).center_y().child(
        el().grow(1.0).min_w(0.0).child(
            kit::Input::new(&f.token)
                .placeholder("Optional")
                .disabled(busy)
                .on_edit(|s: &mut App| &mut s.ui.settings.devices.fields.token)
                .on_change(|s: &mut App, _: &mut rune::Tx| token_edited(s))
                .build(t)
                .label("Expo access token")
                .single_line()
                .secure(),
        ),
    );
    if token != Token::Cleared {
        token_row = token_row.child(
            kit::Button::new("Clear access token")
                .variant("ghost-destructive")
                .size("icon-sm")
                .icon(&icons::X)
                .disabled(busy)
                .on_press(clear_token)
                .build(t),
        );
    }

    let checking = matches!(eas.account, Account::Checking(_));
    let account = match &eas.account {
        Account::SignedIn(account) => format!(
            "Signed in as {} with {}.",
            account["signedInAs"].as_str().unwrap_or(""),
            credential_source(account["credentialSource"].as_str().unwrap_or(""))
        ),
        Account::Failed(error) => error.clone(),
        Account::Checking(_) => "Checking the Expo account…".to_owned(),
        Account::Idle => "Check the Expo account before choosing a project.".to_owned(),
    };
    let account_box = boxed(t).child(
        row()
            .w_full()
            .px(12.0)
            .py(10.0)
            .gap(12.0)
            .center_y()
            .justify_between()
            .child(text(account).font(theme::TEXT_XS).color(muted).wrap().role("status").shrink())
            .child(
                kit::Button::new(if checking { "Checking…" } else { "Check account" })
                    .variant("outline")
                    .size("sm")
                    .disabled(busy || checking)
                    .on_press(check_account)
                    .build(t),
            ),
    );

    let choices = app_choices(eas);
    let labels: Vec<String> = choices.iter().map(|(label, _)| label.clone()).collect();
    let current = choices.iter().find(|(_, id)| *id == eas.app_id).map(|(label, _)| label.clone()).unwrap_or_default();
    let project_select = |open: bool| {
        let choices = choices.clone();
        kit::Select::new("eas-device-host-project", labels.as_slice())
            .current(Some(current.as_str()))
            .label("EAS project")
            .width(464.0)
            .size("sm")
            .disabled(busy)
            .open(open)
            .on_toggle(|s: &mut App| {
                if let Some(eas) = eas_mut(s) {
                    eas.app_menu = !eas.app_menu;
                }
            })
            .on_close(|s: &mut App| {
                if let Some(eas) = eas_mut(s) {
                    eas.app_menu = false;
                }
            })
            .on_choose(move |s: &mut App, label: &str, _: &mut rune::Tx| {
                if let Some(eas) = eas_mut(s) {
                    if let Some((_, id)) = choices.iter().find(|(choice, _)| choice == label) {
                        eas.app_id = id.clone();
                    }
                    eas.app_menu = false;
                }
            })
    };

    let limits = row()
        .w_full()
        .gap(12.0)
        .items_start()
        .child(limit(t, busy, "Sessions at once", &f.sessions, sessions_field))
        .child(limit(t, busy, "Session minutes", &f.session_minutes, session_minutes_field))
        .child(limit(t, busy, "Idle minutes", &f.idle_minutes, idle_minutes_field));
    let restart = if eas.is_new { "" } else { " Saving restarts this host, which stops its running sessions." };

    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(field(
            t,
            "Name",
            kit::Input::new(&f.label)
                .disabled(busy)
                .on_edit(|s: &mut App| &mut s.ui.settings.devices.fields.label)
                .on_submit(|s: &mut App, _: &mut rune::Tx| save(s))
                .build(t)
                .label("Name")
                .single_line(),
        ))
        .child(field(t, "Expo access token", token_row))
        .child(account_box)
        .child(
            col()
                .w_full()
                .gap(6.0)
                .child(text("EAS project").font(theme::TEXT_SM).color(t.c.foreground))
                .child(project_select(false).trigger(&k))
                .child(note(
                    t,
                    "Sessions bill the project's Expo account. \"From the thread's project\" reads extra.eas.projectId from the project's app config.",
                )),
        )
        .child(limits)
        .child(note(
            t,
            format!(
                "Expo allows sessions of up to 40 minutes on free accounts and 115 on paid ones, and ends one after the idle minutes without activity, so idle minutes must be smaller than session minutes.{restart}"
            ),
        ));
    if let Some(problem) = problem {
        body = body.child(text(problem).font(theme::TEXT_XS).color(t.c.destructive).wrap().role("alert"));
    }
    if let Some(error) = &editor.error {
        body = body.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap().role("alert"));
    }
    kit::Dialog::new("eas-device-host-editor", if eas.is_new { "Add EAS host" } else { "Edit EAS host" })
        .open(true)
        .description("The environment's server signs in to Expo and starts the sessions. Without a token here it uses EXPO_TOKEN, then its eas login.")
        .on_close(close)
        .child(body)
        .child(footer(t, busy, problem.is_none()))
        .build(t, layers);
    // After the dialog so the list stacks above it.
    if eas.app_menu {
        let _trigger = project_select(true).build(&k, layers);
    }
}

fn sessions_field(s: &mut App) -> &mut TextEdit {
    &mut s.ui.settings.devices.fields.sessions
}

fn session_minutes_field(s: &mut App) -> &mut TextEdit {
    &mut s.ui.settings.devices.fields.session_minutes
}

fn idle_minutes_field(s: &mut App) -> &mut TextEdit {
    &mut s.ui.settings.devices.fields.idle_minutes
}

fn limit(t: &T, busy: bool, label: &str, edit: &TextEdit, pick: fn(&mut App) -> &mut TextEdit) -> El {
    el().grow(1.0).min_w(0.0).child(field(
        t,
        label,
        kit::Input::new(edit).disabled(busy).on_edit(pick).build(t).label(label.to_owned()).single_line(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ssh_drafts_parse_like_the_schema() {
        let host = ssh_host("h1", " Mini ", "me@mini", " ~/.ssh/id ", "2222").unwrap();
        assert_eq!(host, json!({"kind": "ssh", "id": "h1", "label": "Mini", "target": "me@mini", "identityFile": "~/.ssh/id", "port": 2222}));
        assert_eq!(ssh_host("h1", "", "mini", "", "").unwrap()["label"], "mini");
        assert!(ssh_host("h1", "Mini", "", "", "").is_none());
        assert!(ssh_host("h1", "Mini", "-oProxyCommand=x", "", "").is_none());
        assert!(ssh_host("h1", "Mini", "me@mini ", "", "").is_none());
        assert!(ssh_host("h1", "Mini", "mini", "", "0").is_none());
        assert!(ssh_host("h1", "Mini", "mini", "", "70000").is_none());
        assert_eq!(connection_key(" mini", "22 ", ""), connection_key("mini", "22", " "));
        assert_ne!(connection_key("mini", "22", ""), connection_key("mini", "", ""));
    }

    #[test]
    fn eas_tokens_stay_saved_until_replaced_or_cleared() {
        assert_eq!(token_state("", true, false), Token::Saved);
        assert_eq!(token_state("", true, true), Token::Cleared);
        assert_eq!(token_state("", false, false), Token::Cleared);
        assert_eq!(token_state("abc", true, true), Token::New);
        assert!(has_token(&json!({"accessToken": {"source": "env", "name": "EXPO_TOKEN"}})));
        assert!(!has_token(&json!({"accessToken": ""})));

        let saved = json!({"kind": "eas", "id": "e", "label": "EAS", "accessToken": "<redacted>", "appId": "a1", "maxSessionMinutes": 30, "idleMinutes": 10, "maxConcurrentSessions": 2});
        let kept = eas_host(&saved, " Expo ", [1, 40, 5], None, Token::Saved, "");
        assert_eq!(kept["accessToken"], "<redacted>");
        assert_eq!(kept["label"], "Expo");
        assert!(kept.get("appId").is_none());
        assert_eq!((kept["maxConcurrentSessions"].as_i64(), kept["maxSessionMinutes"].as_i64(), kept["idleMinutes"].as_i64()), (Some(1), Some(40), Some(5)));
        assert_eq!(eas_host(&saved, "Expo", [2, 30, 10], Some("a2"), Token::New, "tok")["accessToken"], "tok");
        assert_eq!(eas_host(&saved, "Expo", [2, 30, 10], Some("a2"), Token::Cleared, "")["accessToken"], "");
    }

    #[test]
    fn eas_problems_explain_why_save_is_off() {
        assert_eq!(eas_problem("EAS", 2, 30, 10), None);
        assert_eq!(eas_problem("EAS", 2, 30, 30), Some("Idle minutes must be smaller than session minutes."));
        assert_eq!(eas_problem(" ", 2, 30, 10), Some("Give the host a label and limits within the ranges shown."));
        assert_eq!(eas_problem("EAS", 21, 30, 10), Some("Give the host a label and limits within the ranges shown."));
        assert_eq!(eas_problem("EAS", 2, 0, 10), Some("Give the host a label and limits within the ranges shown."));
        assert_eq!(number("abc"), 0);
    }
}
