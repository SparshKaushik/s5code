//! An agent's request to link this environment to another machine (LinkRequestCard, client-runtime
//! linkRequest.ts). The web's dialog opens inline: pick a machine or address, then access. A code
//! is minted on the target when this window is connected to it, else the user pastes one.

use std::collections::{HashMap, HashSet};

use rune::{El, TextEdit, col, icon, row, text};
use serde_json::{Value, json};

use super::rows::array;
use crate::data::s;
use crate::theme::{self, AlphaMul, T};
use crate::{App, env, icons, kit};

/// The picker's "Another address…" choice.
const ANOTHER: &str = "\u{0}another-address";
const GENERIC: &str = "Could not answer the request. Try again.";
const NO_MATCH: u8 = 4;

#[derive(Default)]
pub struct State {
    /// The pending item whose link form is open; one at a time, like the web's dialog.
    open: Option<String>,
    /// The machine picked, or [`ANOTHER`]; None follows the agent's hint.
    choice: Option<String>,
    address: TextEdit,
    code: TextEdit,
    access: String,
    /// Machines whose minting failed, so the form asks for a code instead.
    mint_failed: HashSet<String>,
    /// Turn item id -> the `peerLinks.answerRequest` in flight.
    sending: HashMap<String, u64>,
    /// A pairing-code mint in flight, with the item and target it answers.
    minting: Option<(u64, Value, Target)>,
    errors: HashMap<String, String>,
    /// `peerLinks.list` in flight, then the environments this one links to already.
    listing: Option<u64>,
    linked: HashSet<String>,
    /// Server URL -> whether this client's session may answer (`access:write`, the
    /// `peerLinks.answerRequest` guard); then the `GET /api/auth/session` in flight.
    can_answer: HashMap<String, bool>,
    session: Option<(u64, String)>,
}

/// The guard's verdict for a session; a failed lookup leaves answering to the server.
fn answer_allowed(session: &Result<Value, Value>) -> bool {
    session.as_ref().map_or(true, |session| crate::settings::session_grants(session, "access:write"))
}

/// A machine this window knows (LinkableMachine).
#[derive(Clone, Debug, PartialEq)]
struct Machine {
    /// Its key in [`env`].
    key: String,
    id: String,
    label: String,
    urls: Vec<String>,
    connected: bool,
    linked: bool,
}

/// What the form links (the web's DialogTarget).
#[derive(Clone, Debug, PartialEq)]
enum Target {
    None,
    Machine(Machine),
    Address,
}

/// What the form asks for and whether Link can be pressed.
#[derive(Debug, PartialEq)]
struct Form {
    asks_address: bool,
    asks_code: bool,
    use_existing: bool,
    ready: bool,
}

/// linkRequestTargetName.
fn target_name(item: &Value) -> String {
    let label = s(item, "label").trim();
    if !label.is_empty() {
        label.to_owned()
    } else if !s(item, "url").is_empty() {
        s(item, "url").to_owned()
    } else {
        "a machine".to_owned()
    }
}

fn picks_machine(item: &Value) -> bool {
    s(item, "environmentId").is_empty() && s(item, "url").is_empty()
}

/// linkRequestDefaultAccess.
fn default_access(item: &Value) -> String {
    item["requestedAccess"].as_str().filter(|a| !a.is_empty()).unwrap_or("approval-required").to_owned()
}

/// ConnectAgentSurface's accessConfig: read only, then the runtime modes.
fn accesses() -> Vec<(&'static str, &'static str, &'static str)> {
    std::iter::once(("read-only", "Read only", "Read projects and threads. Cannot start, message or change anything."))
        .chain(crate::chat::composer::RUNTIME_MODES.iter().map(|(id, label, description, _)| (*id, *label, *description)))
        .collect()
}

fn access_label(access: &str) -> String {
    accesses().into_iter().find(|(id, ..)| *id == access).map_or_else(|| access.to_owned(), |(_, label, _)| label.to_owned())
}

/// mcpClientScopes.
fn scopes(access: &str) -> Vec<&'static str> {
    match access {
        "read-only" => vec!["orchestration:read"],
        "full-access" => vec!["orchestration:read", "orchestration:operate", "settings:write", "source-control:write"],
        _ => vec!["orchestration:read", "orchestration:operate"],
    }
}

/// linkRequestDisplay's one-line outcomes, or None while the actions should show. A request a fork
/// inherited can only be answered in the thread that asked.
fn settled(item: &Value, thread: &str) -> Option<(&'static rune::Icon, String)> {
    let (glyph, label) = match s(item, "linkStatus") {
        "pending" if s(item, "threadId").is_empty() || s(item, "threadId") == thread => return None,
        "pending" => (&icons::LINK, "Waiting for an answer in the original thread".to_owned()),
        "linked" => {
            let name = item["linkedLabel"].as_str().filter(|l| !l.is_empty()).map_or_else(|| target_name(item), str::to_owned);
            let access = item["linkedAccess"].as_str().map(|a| format!(" · {}", access_label(a))).unwrap_or_default();
            (&icons::CHECK, format!("Linked {name}{access}"))
        }
        "declined" => (&icons::MINUS, "Declined".to_owned()),
        "failed" => (&icons::X, format!("Could not link: {}", item["failure"].as_str().filter(|f| !f.is_empty()).unwrap_or("the other environment refused"))),
        _ => (&icons::MINUS, "Request ended".to_owned()),
    };
    Some((glyph, label))
}

fn words(text: &str) -> String {
    let lower = text.to_lowercase();
    let parts: Vec<&str> = lower.split(|c: char| !c.is_alphanumeric()).filter(|w| !w.is_empty()).collect();
    format!(" {} ", parts.join(" "))
}

/// hintRank: exact, prefix, substring, then a hint naming it among other words ("the box on hetzner").
fn hint_rank(label: &str, hint: &str) -> u8 {
    let name = label.trim().to_lowercase();
    let hint = hint.trim().to_lowercase();
    let wanted = ["my ", "our ", "the "].iter().find_map(|p| hint.strip_prefix(p)).map_or(hint.as_str(), str::trim_start);
    if name == wanted {
        0
    } else if name.starts_with(wanted) {
        1
    } else if name.contains(wanted) {
        2
    } else if words(wanted).contains(&words(&name)) {
        3
    } else {
        NO_MATCH
    }
}

/// rankMachinesByHint: best match first, each rank in catalog order.
fn rank(mut machines: Vec<Machine>, hint: &str) -> Vec<Machine> {
    if !hint.trim().is_empty() {
        machines.sort_by_key(|m| hint_rank(&m.label, hint));
    }
    machines
}

/// linkRequestInitialMachine: the machine the request names, else the hint's best match.
fn initial_machine(item: &Value, machines: &[Machine]) -> Option<String> {
    if !s(item, "environmentId").is_empty() {
        return Some(s(item, "environmentId").to_owned());
    }
    let hint = s(item, "hint");
    if hint.trim().is_empty() {
        return None;
    }
    rank(machines.to_vec(), hint).into_iter().next().filter(|m| hint_rank(&m.label, hint) < NO_MATCH).map(|m| m.id)
}

/// resolveTarget.
fn resolve_target(item: &Value, machines: &[Machine], choice: Option<&str>) -> Target {
    let (named, url) = (s(item, "environmentId"), s(item, "url"));
    if named.is_empty() && !url.is_empty() {
        return Target::Address;
    }
    let chosen = if named.is_empty() {
        match choice {
            None => return Target::None,
            Some(ANOTHER) => return Target::Address,
            Some(chosen) => chosen,
        }
    } else {
        named
    };
    let found = machines.iter().find(|m| m.id == chosen);
    let mut urls = found.map(|m| m.urls.clone()).unwrap_or_default();
    if !url.is_empty() && !urls.iter().any(|u| u == url) {
        urls.push(url.to_owned());
    }
    Target::Machine(Machine {
        key: found.map(|m| m.key.clone()).unwrap_or_default(),
        id: chosen.to_owned(),
        label: found.map_or_else(|| target_name(item), |m| m.label.clone()),
        urls,
        connected: found.is_some_and(|m| m.connected),
        linked: found.is_some_and(|m| m.linked),
    })
}

fn form(item: &Value, target: &Target, address: &str, code: &str, can_mint: bool) -> Form {
    let needs_address = match target {
        Target::Address => true,
        Target::Machine(m) => m.urls.is_empty(),
        Target::None => false,
    };
    // An address the agent gave is filled in already; only one the user must type is asked for.
    let asks_address = needs_address && !(!s(item, "url").is_empty() && s(item, "environmentId").is_empty());
    let use_existing = matches!(target, Target::Machine(m) if m.linked);
    let machine_ready = *target != Target::None && (use_existing || !needs_address || !address.trim().is_empty());
    Form {
        asks_address,
        asks_code: !use_existing && !can_mint,
        use_existing,
        ready: machine_ready && (use_existing || can_mint || !code.trim().is_empty()),
    }
}

fn input(item: &Value, thread: &str, answer: Value) -> Value {
    let thread = if s(item, "threadId").is_empty() { thread } else { s(item, "threadId") };
    json!({"threadId": thread, "turnItemId": s(item, "id"), "answer": answer})
}

/// linkRequestAnswerInput for a link: a blank code or address sends nothing.
fn link_input(item: &Value, thread: &str, target: &Target, address: &str, access: &str, code: &str) -> Option<Value> {
    let code = code.trim();
    if code.is_empty() {
        return None;
    }
    let answer = match target {
        Target::None => return None,
        Target::Address => {
            let url = address.trim();
            if url.is_empty() {
                return None;
            }
            json!({"type": "link", "urls": [url], "access": access, "pairingCode": code})
        }
        Target::Machine(m) => {
            let urls = if m.urls.is_empty() { vec![address.trim().to_owned()] } else { m.urls.clone() };
            if urls[0].is_empty() {
                return None;
            }
            json!({"type": "link", "environmentId": m.id, "label": m.label, "urls": urls, "access": access, "pairingCode": code})
        }
    };
    Some(input(item, thread, answer))
}

/// An RPC failure's cause (`[{_tag: "Fail", error}]`) as its typed error.
pub(super) fn failure(cause: &Value) -> &Value {
    cause.as_array().and_then(|c| c.iter().find(|r| s(r, "_tag") == "Fail")).map_or(cause, |r| &r["error"])
}

/// linkRequestFailureMessage: only known errors pass through, so the pairing code never shows.
/// PeerLinkRequestError's message is a getter, so it may arrive as just its reason.
fn failure_message(cause: &Value) -> String {
    let error = failure(cause);
    let message = s(error, "message").trim();
    let known = match s(error, "_tag") {
        "PeerLinkRequestError" if message.is_empty() => match s(error, "reason") {
            "load_failed" => "Could not load the link request.",
            "not_found" => "This link request no longer exists.",
            "already_answered" => "This link request was already answered.",
            "record_failed" => "Could not update the link request.",
            _ => GENERIC,
        },
        "PeerLinkRequestError" | "EnvironmentAuthorizationError" | "PeerPairingCodeError" if !message.is_empty() => message,
        _ => GENERIC,
    };
    known.to_owned()
}

/// PeerPairingCodeError's message for a failed mint.
fn mint_failure(error: &Value) -> &'static str {
    let error = error.to_string();
    if error.contains("EnvironmentScopeRequiredError") || error.contains("EnvironmentOperationForbiddenError") {
        "Your session there cannot create pairing codes. Paste a code from there instead."
    } else {
        "Could not get a pairing code from there. Paste a code from there instead."
    }
}

/// The other environments this window is paired with (linkableMachines, minus T3 Connect discovery).
fn machines(app: &App) -> Vec<Machine> {
    let linked = &app.ui.chat.timeline.links.linked;
    env::keys(app)
        .into_iter()
        .filter(|key| *key != app.ui.env.active)
        .filter_map(|key| {
            let st = env::source(app, &key)?.state();
            let config = st.config.as_ref();
            let id = config.and_then(|c| c["environment"]["environmentId"].as_str()).filter(|i| !i.is_empty()).map_or_else(|| key.clone(), str::to_owned);
            if id.is_empty() {
                return None;
            }
            let mut urls: Vec<String> = config.map(|c| array(c, "directEndpoints").iter().map(|e| s(e, "httpBaseUrl").to_owned()).filter(|u| !u.is_empty()).collect()).unwrap_or_default();
            if let Some(saved) = app.ui.env.saved.iter().find(|e| e.id == key)
                && !urls.contains(&saved.base)
            {
                urls.push(saved.base.clone());
            }
            Some(Machine {
                label: env::label(app, &key),
                connected: matches!(st.conn, s5_net::Conn::Open) && st.shell_ready,
                linked: linked.contains(&id),
                key,
                id,
                urls,
            })
        })
        .collect()
}

fn can_mint(ui: &State, machine: &Machine) -> bool {
    machine.connected && !ui.mint_failed.contains(&machine.id)
}

fn selected(ui: &State, item: &Value, machines: &[Machine]) -> Option<String> {
    ui.choice.clone().or_else(|| initial_machine(item, machines))
}

pub(super) fn open(app: &mut App, item: &Value) {
    let id = s(item, "id").to_owned();
    let ui = &mut app.ui.chat.timeline.links;
    ui.open = Some(id.clone());
    ui.choice = None;
    ui.address = TextEdit::new(s(item, "url"));
    ui.code.clear();
    ui.access = default_access(item);
    ui.errors.remove(&id);
    if ui.listing.is_none()
        && let Some(net) = app.src.net()
    {
        let request = net.request("peerLinks.list", json!({}));
        app.ui.chat.timeline.links.listing = Some(request);
    }
}

/// Every close resets, so a reopened form never shows the last code.
fn close(app: &mut App) {
    let ui = &mut app.ui.chat.timeline.links;
    if let Some(id) = ui.open.take() {
        ui.errors.remove(&id);
    }
    ui.code.clear();
}

fn send(app: &mut App, id: &str, input: Value) {
    let Some(net) = app.src.net() else { return };
    let ui = &mut app.ui.chat.timeline.links;
    if ui.sending.contains_key(id) {
        return;
    }
    let request = net.request("peerLinks.answerRequest", input);
    ui.sending.insert(id.to_owned(), request);
    ui.errors.remove(id);
}

fn submit(app: &mut App, item: &Value, thread: &str) {
    let id = s(item, "id").to_owned();
    let ui = &app.ui.chat.timeline.links;
    if ui.sending.contains_key(&id) || ui.minting.is_some() {
        return;
    }
    let machines = rank(machines(app), s(item, "hint"));
    let target = resolve_target(item, &machines, selected(ui, item, &machines).as_deref());
    match &target {
        Target::None => {}
        Target::Machine(m) if m.linked => send(app, &id, input(item, thread, json!({"type": "use-existing", "environmentId": m.id}))),
        Target::Machine(m) if can_mint(ui, m) => {
            let access = ui.access.clone();
            let Some(net) = env::source_mut(app, &m.key).and_then(|src| src.net()) else { return };
            let request = net.http_request("POST", "/api/auth/pairing-token", json!({"label": "T3 Code link", "scopes": scopes(&access)}));
            let ui = &mut app.ui.chat.timeline.links;
            ui.minting = Some((request, item.clone(), target.clone()));
            ui.errors.remove(&id);
        }
        _ => {
            if let Some(answer) = link_input(item, thread, &target, &ui.address.text, &ui.access, &ui.code.text) {
                send(app, &id, answer);
            }
        }
    }
}

pub fn poll(app: &mut App) -> bool {
    let thread = match app.frame_route() {
        crate::Route::Thread(id) => id.clone(),
        _ => String::new(),
    };
    let asks = app.state().thread_views.get(&thread).is_some_and(|v| {
        array(&v.projection, "visibleTurnItems").iter().any(|r| s(&r["item"], "type") == "link_request" && settled(&r["item"], &thread).is_none())
    });
    let Some(net) = app.src.net() else { return false };
    let ui = &mut app.ui.chat.timeline.links;
    let mut changed = false;
    let base = net.base_url().to_owned();
    if asks && ui.session.is_none() && !ui.can_answer.contains_key(&base) {
        ui.session = Some((net.http_request("GET", "/api/auth/session", json!({})), base));
    }
    if let Some((request, base)) = ui.session.take() {
        match net.take_reply(request) {
            Some(reply) => {
                changed = true;
                ui.can_answer.insert(base, answer_allowed(&reply));
            }
            None => ui.session = Some((request, base)),
        }
    }
    if let Some(request) = ui.listing
        && let Some(reply) = net.take_reply(request)
    {
        changed = true;
        ui.listing = None;
        if let Ok(list) = reply {
            ui.linked = array(&list, "links").iter().filter(|l| s(l, "status") != "expired").map(|l| s(l, "environmentId").to_owned()).collect();
        }
    }
    let minted = ui.minting.as_ref().and_then(|(request, ..)| net.take_reply(*request));
    if let Some(reply) = minted
        && let Some((_, item, target)) = ui.minting.take()
    {
        changed = true;
        let id = s(&item, "id").to_owned();
        match reply {
            Ok(minted) => {
                if let Some(answer) = link_input(&item, &thread, &target, &ui.address.text, &ui.access, s(&minted, "credential")) {
                    let request = net.request("peerLinks.answerRequest", answer);
                    ui.sending.insert(id, request);
                }
            }
            Err(error) => {
                if let Target::Machine(m) = &target {
                    ui.mint_failed.insert(m.id.clone());
                }
                ui.errors.insert(id, mint_failure(&error).to_owned());
            }
        }
    }
    ui.sending.retain(|id, request| {
        let Some(reply) = net.take_reply(*request) else { return true };
        changed = true;
        match reply {
            Ok(_) if ui.open.as_deref() == Some(id.as_str()) => {
                ui.open = None;
                ui.code.clear();
            }
            Ok(_) => {}
            Err(error) => {
                ui.errors.insert(id.clone(), failure_message(&error));
            }
        }
        false
    });
    changed
}

fn field_label(label: &str, t: &T) -> El {
    text(label).font(theme::XS_MEDIUM).color(t.c.foreground)
}

fn hint(copy: String, t: &T) -> El {
    text(copy).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()
}

fn form_view(app: &App, item: &Value, thread: &str, busy: bool, t: &T) -> El {
    let ui = &app.ui.chat.timeline.links;
    let k = kit::theme(t);
    let id = s(item, "id").to_owned();
    let machines = rank(machines(app), s(item, "hint"));
    let picked = selected(ui, item, &machines);
    let target = resolve_target(item, &machines, picked.as_deref());
    let minting = matches!(&target, Target::Machine(m) if can_mint(ui, m));
    let f = form(item, &target, &ui.address.text, &ui.code.text, minting);
    let name = match &target {
        Target::Machine(m) => m.label.clone(),
        _ => target_name(item),
    };
    let mut body = col().w_full().gap(12.0).child(hint("Agents here can then launch, message and wait on threads there. Revoke it in that machine's Connections.".into(), t));
    let submit_item = item.clone();
    let submit_thread = thread.to_owned();
    let go = move |app: &mut App| submit(app, &submit_item, &submit_thread);
    if picks_machine(item) {
        let mut picker = col().w_full().gap(8.0).child(field_label("Machine", t));
        let choices = machines.iter().map(|m| {
            let note = match (m.linked, m.connected) {
                (true, _) => "Linked already",
                (false, false) => "Not connected from this app",
                (false, true) => "",
            };
            (m.id.clone(), m.label.clone(), note)
        });
        for (choice, label, note) in choices.chain(std::iter::once((ANOTHER.to_owned(), "Another address…".to_owned(), ""))) {
            let checked = picked.as_deref() == Some(choice.as_str());
            let mut radio = kit::RadioItem::new(label, checked).disabled(busy);
            if !note.is_empty() {
                radio = radio.description(note);
            }
            let item_id = id.clone();
            picker = picker.child(
                radio
                    .on_select(move |app: &mut App| {
                        let ui = &mut app.ui.chat.timeline.links;
                        ui.choice = Some(choice.clone());
                        ui.errors.remove(&item_id);
                    })
                    .build(&k),
            );
        }
        body = body.child(picker);
    }
    if f.asks_address {
        let submit = go.clone();
        let mut field = col().w_full().gap(6.0).child(field_label("Address", t)).child(
            kit::Input::new(&ui.address)
                .placeholder("https://machine.tailnet.ts.net")
                .disabled(busy)
                .on_edit(|app: &mut App| &mut app.ui.chat.timeline.links.address)
                .on_submit(move |app: &mut App, _| submit(app))
                .build(t)
                .single_line()
                .label("Address"),
        );
        if let Target::Machine(m) = &target {
            field = field.child(hint(format!("This app has no address for {} that another machine can reach. Enter its https or Tailscale address.", m.label), t));
        }
        body = body.child(field);
    }
    if f.use_existing {
        body = body.child(hint(format!("This environment is already linked to {name}. The agent can use that link as it is; change its access in Settings → Connections."), t));
    } else {
        if f.asks_code {
            let submit = go.clone();
            body = body.child(
                col()
                    .w_full()
                    .gap(6.0)
                    .child(field_label("Pairing code", t))
                    .child(
                        kit::Input::new(&ui.code)
                            .placeholder("Paste the code")
                            .disabled(busy)
                            .on_edit(|app: &mut App| &mut app.ui.chat.timeline.links.code)
                            .on_submit(move |app: &mut App, _| submit(app))
                            .build(t)
                            .single_line()
                            .label("Pairing code"),
                    )
                    .child(hint(format!("Create a link on {name}: Settings → Connections → Create link, then paste its code here."), t)),
            );
        }
        let mut picker = col().w_full().gap(8.0).child(field_label("Access", t));
        for (access, label, description) in accesses() {
            picker = picker.child(
                kit::RadioItem::new(label, ui.access == access)
                    .description(description)
                    .disabled(busy)
                    .on_select(move |app: &mut App| app.ui.chat.timeline.links.access = access.to_owned())
                    .build(&k),
            );
        }
        body = body.child(picker);
    }
    if let Some(error) = ui.errors.get(&id) {
        body = body.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
    }
    let label = if busy {
        "Linking…"
    } else if f.use_existing {
        "Use this link"
    } else {
        "Link"
    };
    body.child(
        row()
            .w_full()
            .gap(8.0)
            .justify_end()
            .child(kit::Button::new("Cancel").variant("outline").size("xs").disabled(busy).on_press(close).build(t))
            .child(kit::Button::new(label).size("xs").disabled(busy || !f.ready).on_press(go).build(t)),
    )
}

pub fn view(app: &App, item: &Value, thread: &str, t: &T) -> El {
    if let Some((glyph, label)) = settled(item, thread) {
        return row()
            .gap(12.0)
            .px(4.0)
            .py(4.0)
            .center_y()
            .child(icon(glyph, 14.0).color(t.c.icon_muted))
            .child(text(label).font(theme::TEXT_SM).color(t.c.muted_foreground).truncate().shrink());
    }
    let id = s(item, "id").to_owned();
    let ui = &app.ui.chat.timeline.links;
    let busy = ui.sending.contains_key(&id) || ui.minting.as_ref().is_some_and(|(_, minting, _)| s(minting, "id") == id);
    let title = if picks_machine(item) { "Link a machine to this environment".to_owned() } else { format!("Link {} to this environment", target_name(item)) };
    let mut head = col().w_full().gap(4.0).child(text(title).font(theme::SM_MEDIUM).color(t.c.foreground));
    if !s(item, "reason").trim().is_empty() {
        head = head.child(text(s(item, "reason").to_owned()).font(theme::TEXT_SM).color(t.c.muted_foreground).selectable(true).wrap());
    }
    if !s(item, "label").is_empty() && !s(item, "url").is_empty() {
        head = head.child(text(s(item, "url").to_owned()).font(theme::TEXT_XS).color(t.c.muted_foreground.alpha_mul(0.8)).truncate());
    }
    let card = col().w_full().gap(12.0).padding(16.0).radius(12.0).border(1.0, t.c.border.alpha_mul(0.6)).fill(t.c.card).child(head);
    if ui.open.as_deref() == Some(id.as_str()) {
        return card.child(form_view(app, item, thread, busy, t));
    }
    let errors = ui.errors.get(&id).map(|error| text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
    let allowed = match &app.src {
        crate::data::Source::Live(net) => ui.can_answer.get(net.base_url()).copied().unwrap_or(true),
        crate::data::Source::Fixture(_) => true,
    };
    if !allowed {
        return card.children(errors);
    }
    let (decline_item, decline_thread, open_item) = (item.clone(), thread.to_owned(), item.clone());
    card.children(errors).child(
        row()
            .w_full()
            .gap(8.0)
            .justify_end()
            .child(
                kit::Button::new("Decline")
                    .variant("ghost-muted")
                    .size("xs")
                    .disabled(busy)
                    .on_press(move |app: &mut App| {
                        let id = s(&decline_item, "id").to_owned();
                        send(app, &id, input(&decline_item, &decline_thread, json!({"type": "decline"})));
                    })
                    .build(t),
            )
            .child(
                kit::Button::new(if picks_machine(item) { "Choose machine…" } else { "Review link…" })
                    .size("xs")
                    .disabled(busy)
                    .on_press(move |app: &mut App| open(app, &open_item))
                    .build(t),
            ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn machine(id: &str, label: &str) -> Machine {
        Machine { key: id.into(), id: id.into(), label: label.into(), urls: vec![format!("https://{id}.ts.net")], connected: true, linked: false }
    }

    #[test]
    fn only_sessions_that_manage_access_answer() {
        let session = |scopes: Value| -> Result<Value, Value> { Ok(json!({"authenticated": true, "scopes": scopes})) };
        assert!(answer_allowed(&session(json!(["orchestration:read", "access:write"]))));
        assert!(!answer_allowed(&session(json!(["orchestration:read", "orchestration:operate"]))));
        assert!(!answer_allowed(&Ok(json!({"authenticated": false}))));
        assert!(answer_allowed(&Err(json!({"message": "HTTP 500"}))));
    }

    #[test]
    fn settled_requests_say_how_they_ended() {
        let pending = json!({"linkStatus": "pending", "threadId": "origin"});
        assert!(settled(&pending, "origin").is_none());
        assert_eq!(settled(&pending, "fork").map(|(_, l)| l).as_deref(), Some("Waiting for an answer in the original thread"));
        let linked = json!({"linkStatus": "linked", "label": "box", "linkedAccess": "read-only"});
        assert_eq!(settled(&linked, "t").map(|(_, l)| l).as_deref(), Some("Linked box · Read only"));
        let linked = json!({"linkStatus": "linked", "linkedLabel": "Studio", "linkedAccess": "full-access"});
        assert_eq!(settled(&linked, "t").map(|(_, l)| l).as_deref(), Some("Linked Studio · Full access"));
        assert_eq!(settled(&json!({"linkStatus": "failed"}), "t").map(|(_, l)| l).as_deref(), Some("Could not link: the other environment refused"));
        assert_eq!(settled(&json!({"linkStatus": "cancelled"}), "t").map(|(_, l)| l).as_deref(), Some("Request ended"));
    }

    #[test]
    fn the_hint_picks_the_best_named_machine() {
        let machines = vec![machine("a", "Laptop"), machine("b", "Hetzner box"), machine("c", "box")];
        assert_eq!(hint_rank("box", "my box"), 0);
        assert_eq!(hint_rank("Hetzner box", "hetz"), 1);
        assert_eq!(hint_rank("Hetzner box", "box"), 2);
        assert_eq!(hint_rank("box", "the box on hetzner"), 3);
        assert_eq!(hint_rank("Laptop", "box"), NO_MATCH);
        let ranked: Vec<_> = rank(machines.clone(), "box").into_iter().map(|m| m.id).collect();
        assert_eq!(ranked, ["c", "b", "a"]);
        assert_eq!(initial_machine(&json!({"hint": "our box"}), &machines).as_deref(), Some("c"));
        assert_eq!(initial_machine(&json!({"hint": "desktop"}), &machines), None);
        assert_eq!(initial_machine(&json!({"environmentId": "z", "hint": "box"}), &machines).as_deref(), Some("z"));
    }

    #[test]
    fn answers_need_a_code_and_an_address() {
        let item = json!({"id": "i", "threadId": "t"});
        let box_ = Target::Machine(machine("b", "box"));
        assert_eq!(link_input(&item, "t", &box_, "", "auto", "  "), None);
        let answer = link_input(&item, "t", &box_, "", "auto", " 123 ").unwrap();
        assert_eq!(answer["answer"], json!({"type": "link", "environmentId": "b", "label": "box", "urls": ["https://b.ts.net"], "access": "auto", "pairingCode": "123"}));
        assert_eq!(link_input(&item, "t", &Target::Address, " ", "auto", "1"), None);
        assert_eq!(link_input(&item, "t", &Target::Address, " https://x ", "auto", "1").unwrap()["answer"]["urls"], json!(["https://x"]));
        let mut bare = machine("b", "box");
        bare.urls.clear();
        assert_eq!(link_input(&item, "t", &Target::Machine(bare.clone()), "https://y", "auto", "1").unwrap()["answer"]["urls"], json!(["https://y"]));
        // A machine without an address asks for one; a linked machine needs nothing more.
        let asked = form(&item, &Target::Machine(bare.clone()), "", "", true);
        assert!(asked.asks_address && !asked.asks_code && !asked.ready);
        bare.linked = true;
        assert_eq!(form(&item, &Target::Machine(bare), "", "", false), Form { asks_address: true, asks_code: false, use_existing: true, ready: true });
        // The agent's own address is filled in, never asked for.
        let by_url = json!({"id": "i", "url": "https://x"});
        assert!(!form(&by_url, &Target::Address, "https://x", "1", false).asks_address);
    }

    #[test]
    fn targets_follow_the_request_then_the_pick() {
        let machines = vec![machine("b", "box")];
        assert_eq!(resolve_target(&json!({"url": "https://x"}), &machines, Some("b")), Target::Address);
        assert_eq!(resolve_target(&json!({}), &machines, None), Target::None);
        assert_eq!(resolve_target(&json!({}), &machines, Some(ANOTHER)), Target::Address);
        let Target::Machine(named) = resolve_target(&json!({"environmentId": "b", "url": "https://x"}), &machines, None) else { panic!() };
        assert_eq!(named.urls, ["https://b.ts.net", "https://x"]);
        let Target::Machine(unknown) = resolve_target(&json!({"environmentId": "q", "label": "Studio"}), &machines, None) else { panic!() };
        assert_eq!((unknown.label.as_str(), unknown.connected), ("Studio", false));
    }

    #[test]
    fn only_known_errors_reach_the_card() {
        let cause = |error: Value| json!([{"_tag": "Fail", "error": error}]);
        assert_eq!(failure_message(&cause(json!({"_tag": "PeerLinkRequestError", "reason": "already_answered"}))), "This link request was already answered.");
        assert_eq!(failure_message(&cause(json!({"_tag": "PeerPairingCodeError", "message": "Code expired"}))), "Code expired");
        assert_eq!(failure_message(&json!({"_tag": "ParseError", "message": "pairingCode: 123"})), GENERIC);
        assert!(mint_failure(&json!({"_tag": "EnvironmentScopeRequiredError"})).starts_with("Your session there"));
        assert!(mint_failure(&json!({"message": "HTTP 500"})).starts_with("Could not get"));
        assert_eq!(scopes("read-only"), ["orchestration:read"]);
        assert_eq!(scopes("full-access").len(), 4);
    }
}
