//! Message actions (TimelineUserMessageRow, AssistantMessageMeta, useTimelineRewind): a prompt's
//! timestamp, "Edit from here" and copy; a reply's copy, "Fork from this response" and timestamp.

use super::rows;
use crate::data::{Zone, iso_ms, s};
use crate::theme::{self, AlphaMul, T};
use crate::{App, Route, icons};
use rune::{Cursor, El, icon, row, text};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::mpsc::{Receiver, TryRecvError};

const FAILED: &str = "Failed to revert thread state.";
const CHANGED: &str = "The thread has changed. Select a message to rewind again.";
const TIMEOUT_MS: i64 = 120_000;
/// waitForThreadShell's default before a fork reports that it never arrived.
const FORK_SHELL_MS: i64 = 5_000;
const FORK_LOST: &str = "The fork was created, but its thread data did not reach this client. Reconnect and try opening it from the sidebar.";
/// PROVIDER_SEND_TURN_MAX_ATTACHMENTS.
const MAX_ATTACHMENTS: usize = 100;
const BOOTSTRAP_PROMPT: &str =
    "[User attached one or more files without additional text. Respond using the conversation context and the attached files.]";

/// A prompt waiting on the "Edit from here?" dialog.
#[derive(Clone)]
pub struct Rewind {
    thread: String,
    text: String,
    records: Vec<Value>,
    attachments: Vec<Value>,
    scope: String,
    checkpoint: String,
}

/// What a sent rollback waits for in the thread projection (waitForRevertedMessage).
struct Watch {
    command: String,
    turn_count: i64,
    /// The latest settled run after the target, which the rollback marks rolled back.
    witness: String,
    thread: String,
    provider_thread: Option<String>,
    instance: Option<String>,
    /// Provider threads before the rollback; rewinding Claude to the start makes a new one.
    known: Vec<String>,
}

enum Step {
    /// `assets.createUrl` for each attachment, in order, and the URL once it answers.
    Urls(Vec<(u64, Option<String>)>),
    Download(Receiver<Result<Vec<String>, String>>),
    /// The rollback until the server accepts it (`rpc` taken), then until the projection settles.
    Rollback { rpc: Option<u64>, watch: Watch, started: i64 },
}

struct Pending {
    rewind: Rewind,
    restore_files: bool,
    /// The message's attachments, downloaded for the composer.
    files: Vec<String>,
    step: Step,
}

enum Outcome {
    Waiting,
    Moved,
    Failed(String),
    Done,
}

#[derive(Default)]
pub struct State {
    /// The "Edit from here?" dialog by thread, with why it can't go ahead yet.
    confirm: HashMap<String, (Rewind, Option<String>)>,
    /// Rewinds in flight, by thread.
    rewinds: HashMap<String, Pending>,
    /// In-flight forks, by RPC: the source thread and the thread to open once it lands.
    forks: HashMap<u64, (String, String)>,
    /// Accepted forks waiting for their thread to reach this client: source, target, since.
    landing: Vec<(String, String, i64)>,
    /// A failed revert or fork, by thread.
    errors: HashMap<String, String>,
}

const DAY: i64 = 86_400_000;

/// "9:41 AM" or, under the 24-hour setting, "09:41", for a wall-clock instant.
fn clock(wall: i64, hour12: bool) -> String {
    let mins = wall.rem_euclid(DAY) / 60_000;
    let (h, m) = (mins / 60, mins % 60);
    if hour12 { format!("{}:{m:02} {}", if h % 12 == 0 { 12 } else { h % 12 }, if h < 12 { "AM" } else { "PM" }) } else { format!("{h:02}:{m:02}") }
}

/// "9:41 AM", "yesterday at 9:41 AM" or "3/14 9:41 AM" (formatDayAwareTimestamp) in `zone`.
pub fn timestamp(iso: &str, now: i64, zone: Zone, hour12: bool) -> String {
    let Some(ms) = iso_ms(iso).map(|ms| zone.wall(ms)) else { return String::new() };
    let now = zone.wall(now);
    let time = clock(ms, hour12);
    match now.div_euclid(DAY) - ms.div_euclid(DAY) {
        ..=0 => time,
        1 => format!("yesterday at {time}"),
        _ => {
            let (year, month, day) = civil(ms.div_euclid(DAY));
            let (this_year, _, _) = civil(now.div_euclid(DAY));
            if year == this_year { format!("{month}/{day} {time}") } else { format!("{month}/{day}/{} {time}", year % 100) }
        }
    }
}

/// formatUpcomingTimestamp: "2:16 PM", "tomorrow at 2:16 PM" or "11/12 2:16 PM".
pub fn upcoming(iso: &str, now: i64, zone: Zone, hour12: bool) -> String {
    let Some(at) = iso_ms(iso) else { return String::new() };
    let (wall, today) = (zone.wall(at), zone.wall(now).div_euclid(DAY));
    let time = clock(wall, hour12);
    match wall.div_euclid(DAY) - today {
        days if days < 0 => timestamp(iso, now, zone, hour12),
        0 => time,
        1 => format!("tomorrow at {time}"),
        _ => {
            let ((year, month, day), (this_year, _, _)) = (civil(wall.div_euclid(DAY)), civil(today));
            if year == this_year { format!("{month}/{day} {time}") } else { format!("{month}/{day}/{} {time}", year % 100) }
        }
    }
}

/// The machine's IANA zone name, as the web's `Intl` resolves it ("UTC" when it has none).
pub fn zone_name() -> &'static str {
    static NAME: std::sync::OnceLock<String> = std::sync::OnceLock::new();
    NAME.get_or_init(|| {
        let from_path = |p: &str| p.split_once("zoneinfo/").map(|(_, name)| name.to_owned()).filter(|n| !n.is_empty());
        if let Some(tz) = std::env::var("TZ").ok().map(|t| t.trim_start_matches(':').to_owned()).filter(|t| !t.is_empty()) {
            let named = if tz.starts_with('/') { from_path(tz.as_str()) } else { Some(tz) };
            return named.unwrap_or_else(|| "UTC".to_owned());
        }
        std::fs::read_link("/etc/localtime")
            .ok()
            .and_then(|p| from_path(&*p.to_string_lossy()))
            .or_else(|| std::fs::read_to_string("/etc/timezone").ok().map(|f| f.trim().to_owned()).filter(|f| f.contains('/')))
            .unwrap_or_else(|| "UTC".to_owned())
    })
}

/// "9:41 AM, 3rd March 2026" (formatChatTimestampTooltip).
pub fn full_timestamp(iso: &str, zone: Zone, hour12: bool) -> String {
    const MONTHS: [&str; 12] = ["January", "February", "March", "April", "May", "June", "July", "August", "September", "October", "November", "December"];
    let Some(ms) = iso_ms(iso).map(|ms| zone.wall(ms)) else { return String::new() };
    let (year, month, day) = civil(ms.div_euclid(DAY));
    let suffix = match (day % 10, day % 100) {
        (_, 11..=13) => "th",
        (1, _) => "st",
        (2, _) => "nd",
        (3, _) => "rd",
        _ => "th",
    };
    format!("{}, {day}{suffix} {} {year}", clock(ms, hour12), MONTHS[(month - 1) as usize])
}

/// The `timestampFormat` setting; "locale" follows the system's clock.
pub fn hour12(app: &App) -> bool {
    crate::desktop::locale::hour12(crate::settings::value(app, "timestampFormat").as_str())
}

/// Days since 1970-01-01 to (year, month, day), Howard Hinnant's civil_from_days.
fn civil(days: i64) -> (i64, i64, i64) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    (yoe + era * 400 + i64::from(month <= 2), month, day)
}

fn ghost(t: &T, key: String, glyph: &'static rune::Icon, label: &str) -> El {
    row()
        .key(key)
        .size(24.0, 24.0)
        .center()
        .radius(4.0)
        .hover(t.c.muted)
        .focusable(true)
        .role("button")
        .cursor(Cursor::Pointer)
        .label(label.to_owned())
        .help(label.to_owned())
        .child(icon(glyph, 12.0).color(t.c.muted_foreground))
}

/// The first of `keys` the item carries: turn items date themselves with startedAt and
/// completedAt, conversation messages with createdAt and updatedAt.
fn first_time<'a>(item: &'a Value, keys: &[&str]) -> &'a str {
    keys.iter().map(|k| s(item, k)).find(|v| !v.is_empty()).unwrap_or_default()
}

pub(super) fn stamp(app: &App, iso: &str, t: &T) -> El {
    let h12 = hour12(app);
    text(timestamp(iso, app.now_ms, Zone::Local, h12)).font(theme::TEXT_XS).color(t.c.muted_foreground).help(full_timestamp(iso, Zone::Local, h12))
}

fn running(app: &App, thread: &str) -> bool {
    app.state().thread_views.get(thread).is_some_and(|v| {
        rows::array(&v.projection, "runs").iter().any(|r| matches!(s(r, "status"), "preparing" | "queued" | "starting" | "running" | "waiting"))
    })
}

/// A rewind of `thread` is in flight: its prompts can't be rewound again and Send waits
/// ("Rewinding conversation").
pub fn rewinding(app: &App, thread: &str) -> bool {
    app.ui.chat.timeline.actions.rewinds.contains_key(thread)
}

/// Whether the thread's provider can rewind its conversation (supportsConversationRollback).
fn rolls_back(app: &App) -> bool {
    let composer = crate::chat::composer::ctx(app);
    let instance = composer.thread.map(|t| s(&t["modelSelection"], "instanceId")).unwrap_or_default();
    crate::chat::composer::provider(app, instance).is_none_or(|p| p["supportsConversationRollback"] != false)
}

/// Why the environment can't take a revert now, when it is not connected.
fn offline(app: &App) -> Option<String> {
    (app.state().conn != s5_net::Conn::Open)
        .then(|| format!("Reconnect {} before reverting checkpoints.", crate::env::label(app, &app.ui.env.active)))
}

/// The row under a prompt: timestamp, "Edit from here" when it can be rewound, copy.
pub fn user(app: &App, thread: &str, item: &Value, targets: &HashMap<String, (String, String)>, t: &T) -> El {
    let mut buttons = row().gap(2.0).center_y();
    if let Some((scope, checkpoint)) = targets.get(s(item, "id")) {
        let edit = ghost(t, format!("edit-from-{}", s(item, "id")), &icons::PENCIL, "Edit from here");
        buttons = buttons.child(if running(app, thread) || rewinding(app, thread) {
            edit.disabled(true).opacity(0.64)
        } else {
            let rewind = Rewind {
                thread: thread.to_owned(),
                text: s(item, "text").to_owned(),
                records: rows::array(&item["context"], "records").to_vec(),
                attachments: rows::array(item, "attachments").to_vec(),
                scope: scope.clone(),
                checkpoint: checkpoint.clone(),
            };
            edit.on_press(move |app: &mut App| ask(app, &rewind))
        });
    }
    buttons = buttons.child(super::copy_button(app, s(item, "text"), t, "Copy message"));
    row().gap(8.0).center_y().child(stamp(app, first_time(item, &["createdAt", "startedAt", "completedAt"]), t)).child(buttons)
}

fn ask(app: &mut App, rewind: &Rewind) {
    let blocked = if !rolls_back(app) {
        Some("This provider does not support reverting conversation history. Start a new thread instead.".to_owned())
    } else if let Some(offline) = offline(app) {
        Some(offline)
    } else if running(app, &rewind.thread) {
        Some("Interrupt the current turn before reverting checkpoints.".to_owned())
    } else {
        None
    };
    let state = &mut app.ui.chat.timeline.actions;
    match blocked {
        Some(message) => {
            state.errors.insert(rewind.thread.clone(), message);
        }
        None => {
            state.errors.remove(&rewind.thread);
            state.confirm.insert(rewind.thread.clone(), (rewind.clone(), None));
        }
    }
}

/// An item's own time (TimelineRowTimestamp), when it has one.
pub(super) fn item_stamp(app: &App, item: &Value, t: &T) -> Option<El> {
    let iso = first_time(item, &["createdAt", "startedAt", "updatedAt", "completedAt"]);
    (!iso.is_empty()).then(|| stamp(app, iso, t))
}

/// The row under a finished reply: an unfinished reply's status, copy, fork, timestamp.
pub fn assistant(app: &App, thread: &str, item: &Value, t: &T) -> El {
    let streaming = item["streaming"] == true;
    let mut out = row().gap(8.0).center_y();
    let status = s(item, "status");
    if !status.is_empty() && status != "completed" {
        out = out.child(
            text(status.to_owned())
                .font(theme::mono(theme::TEXT_3XS))
                .color(t.c.muted_foreground)
                .px(6.0)
                .py(2.0)
                .radius(theme::radius::FULL)
                .border(1.0, t.c.border.alpha_mul(0.7)),
        );
    }
    if !streaming {
        out = out.child(super::copy_button(app, s(item, "text"), t, "Copy message"));
    }
    let run = s(item, "runId");
    if !run.is_empty() && !streaming && item["visibility"] != "inherited" {
        let (thread, run) = (thread.to_owned(), run.to_owned());
        let busy = !app.ui.chat.timeline.actions.forks.is_empty() || !app.ui.chat.timeline.actions.landing.is_empty();
        let fork = ghost(t, format!("fork-from-{}", s(item, "id")), &icons::GIT_FORK, "Fork from this response");
        out = out.child(if busy { fork.opacity(0.64) } else { fork.on_press(move |app: &mut App| fork_from(app, &thread, &run)) });
    }
    if !streaming {
        out = out.child(stamp(app, first_time(item, &["updatedAt", "completedAt", "createdAt", "startedAt"]), t));
    }
    out
}

fn fork_from(app: &mut App, thread: &str, run: &str) {
    let title = app.state().threads.iter().find(|t| s(t, "id") == thread).map(|t| s(t, "title").to_owned()).unwrap_or_default();
    let target = s5_net::uuid();
    let Some(net) = app.src.net() else { return };
    let rpc = net.dispatch(json!({
        "type": "thread.fork", "commandId": s5_net::uuid(), "createdBy": "user", "creationSource": "web",
        "sourceThreadId": thread, "targetThreadId": target, "sourcePoint": {"type": "run", "runId": run},
        "title": format!("{title} fork"),
    }));
    app.ui.chat.timeline.actions.errors.remove(thread);
    app.ui.chat.timeline.actions.forks.insert(rpc, (thread.to_owned(), target));
}

/// A failed request's message, else `fallback`.
fn failure(cause: &Value, fallback: &str) -> String {
    let reasons = cause.as_array().map(Vec::as_slice).unwrap_or(std::slice::from_ref(cause));
    reasons
        .iter()
        .find_map(|reason| {
            let error = if reason["_tag"] == "Fail" { &reason["error"] } else { reason };
            error.as_str().or(error["message"].as_str()).filter(|m| !m.trim().is_empty()).map(str::to_owned)
        })
        .unwrap_or_else(|| fallback.to_owned())
}

/// "Revert files too" / "Revert and keep changes" (rewindToMessage): the message's images come
/// down first, then the rollback goes out, and the prompt returns once the projection shows it.
fn rewind(app: &mut App, thread: &str, restore_files: bool) {
    let Some((rewind, _)) = app.ui.chat.timeline.actions.confirm.get(thread).cloned() else { return };
    if rewinding(app, thread) {
        return;
    }
    if let Some(offline) = offline(app) {
        if let Some(confirm) = app.ui.chat.timeline.actions.confirm.get_mut(thread) {
            confirm.1 = Some(offline);
        }
        return;
    }
    app.ui.chat.timeline.actions.confirm.remove(thread);
    app.ui.chat.timeline.actions.errors.remove(thread);
    match start(app, rewind, restore_files) {
        Ok(pending) => {
            app.ui.chat.timeline.actions.rewinds.insert(thread.to_owned(), pending);
        }
        Err(message) => {
            app.ui.chat.timeline.actions.errors.insert(thread.to_owned(), message);
        }
    }
}

fn start(app: &mut App, rewind: Rewind, restore_files: bool) -> Result<Pending, String> {
    if rewind.attachments.iter().any(|a| s(a, "type") != "image" || image_ext(s(a, "mimeType")).is_none()) {
        return Err("This message has an attachment that cannot be restored.".into());
    }
    let key = crate::chat::composer::draft_key(app, &Route::Thread(rewind.thread.clone()));
    if crate::chat::composer::draft_attachment_count(app, &key) + rewind.attachments.len() > MAX_ATTACHMENTS {
        return Err("Make room for this message's attachments in the composer before rewinding.".into());
    }
    let step = if rewind.attachments.is_empty() {
        roll_back(app, &rewind, restore_files)?
    } else {
        let net = app.src.net().ok_or_else(|| "The environment is not connected.".to_owned())?;
        Step::Urls(
            rewind
                .attachments
                .iter()
                .map(|a| {
                    let resource = json!({"_tag": "attachment", "attachmentId": s(a, "id"), "fileName": s(a, "name"), "mimeType": s(a, "mimeType")});
                    (net.request("assets.createUrl", json!({"resource": resource})), None)
                })
                .collect(),
        )
    };
    Ok(Pending { rewind, restore_files, files: Vec::new(), step })
}

/// The run ordinal a checkpoint rewinds to: the run it was taken after, 0 for the thread start.
fn turn_count(p: &Value, checkpoint: &str) -> Option<i64> {
    rows::array(p, "checkpoints").iter().find(|c| s(c, "id") == checkpoint).map(|c| c["appRunOrdinal"].as_i64().unwrap_or(0))
}

fn roll_back(app: &mut App, rewind: &Rewind, restore_files: bool) -> Result<Step, String> {
    let command = s5_net::uuid();
    let projection = &app.state().thread_views.get(&rewind.thread).ok_or_else(|| CHANGED.to_owned())?.projection;
    let turns = turn_count(projection, &rewind.checkpoint).ok_or_else(|| CHANGED.to_owned())?;
    let watch = watch(projection, &command, turns).ok_or_else(|| CHANGED.to_owned())?;
    let net = app.src.net().ok_or_else(|| "The environment is not connected.".to_owned())?;
    let rpc = net.dispatch(json!({
        "type": "checkpoint.rollback", "commandId": command, "threadId": rewind.thread,
        "scopeId": rewind.scope, "checkpointId": rewind.checkpoint, "restoreFiles": restore_files,
    }));
    Ok(Step::Rollback { rpc: Some(rpc), watch, started: crate::data::now_ms() })
}

/// The projection just before a rollback to `turn_count`: None when no settled run after it is
/// left to roll back.
fn watch(p: &Value, command: &str, turn_count: i64) -> Option<Watch> {
    let witness = rows::array(p, "runs")
        .iter()
        .filter(|r| {
            r["ordinal"].as_i64().is_some_and(|o| o > turn_count) && matches!(s(r, "status"), "completed" | "interrupted" | "failed" | "cancelled")
        })
        .max_by_key(|r| r["ordinal"].as_i64())?;
    let thread = &p["thread"];
    let provider_thread = thread["activeProviderThreadId"].as_str().map(str::to_owned);
    let providers = rows::array(p, "providerThreads");
    let instance = providers.iter().find(|t| provider_thread.as_deref() == Some(s(t, "id"))).map(|t| s(t, "providerInstanceId").to_owned());
    Some(Watch {
        command: command.to_owned(),
        turn_count,
        witness: s(witness, "id").to_owned(),
        thread: s(thread, "id").to_owned(),
        provider_thread,
        instance,
        known: providers.iter().map(|t| s(t, "id").to_owned()).collect(),
    })
}

/// Whether a rollback the server has (`accepted`) or hasn't yet acknowledged has landed, failed,
/// or is still going (None).
fn settled(p: &Value, w: &Watch, accepted: bool) -> Option<Result<(), String>> {
    let failure = &p["thread"]["rollbackFailure"];
    if s(failure, "requestId") == w.command {
        return Some(Err(Some(s(failure, "message")).filter(|m| !m.is_empty()).unwrap_or(FAILED).to_owned()));
    }
    if !accepted {
        return None;
    }
    let at_target = |t: &Value| if w.turn_count == 0 { t["lastRunOrdinal"].is_null() } else { t["lastRunOrdinal"].as_i64() == Some(w.turn_count) };
    let rewound = rows::array(p, "providerThreads").iter().any(|t| {
        let id = s(t, "id");
        let fresh_root = w.turn_count == 0
            && t["ownerNodeId"].is_null()
            && s(t, "appThreadId") == w.thread
            && w.instance.as_deref() == Some(s(t, "providerInstanceId"))
            && !w.known.iter().any(|k| k == id);
        (w.provider_thread.as_deref() == Some(id) || fresh_root) && at_target(t)
    });
    let rolled = rows::array(p, "runs").iter().any(|r| s(r, "id") == w.witness && s(r, "status") == "rolled_back");
    (rewound && rolled).then_some(Ok(()))
}

/// The extension the composer reads `mime` back from, for the image types it sends.
fn image_ext(mime: &str) -> Option<&'static str> {
    match mime.to_ascii_lowercase().as_str() {
        "image/png" => Some("png"),
        "image/jpeg" | "image/jpg" => Some("jpg"),
        "image/gif" => Some("gif"),
        "image/webp" => Some("webp"),
        _ => None,
    }
}

/// Downloads `(url, name, mime)` into temporary files, all or none, waking the app when done.
fn download(jobs: Vec<(String, String, String)>, waker: Option<rune::Waker>) -> Receiver<Result<Vec<String>, String>> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let dir = std::env::temp_dir().join(format!("s5-rewind-{}", s5_net::uuid()));
        let result = jobs
            .iter()
            .enumerate()
            .map(|(i, (url, name, mime))| fetch(&dir.join(i.to_string()), url, name, mime).ok_or_else(|| format!("Could not restore attachment: {name}")))
            .collect();
        let _ = tx.send(result);
        if let Some(wake) = waker {
            wake();
        }
    });
    rx
}

fn fetch(dir: &std::path::Path, url: &str, name: &str, mime: &str) -> Option<String> {
    use std::io::Read;
    let response = ureq::get(url).timeout(std::time::Duration::from_secs(30)).call().ok()?;
    let mut bytes = Vec::new();
    response.into_reader().take(20 * 1024 * 1024).read_to_end(&mut bytes).ok()?;
    let base = std::path::Path::new(name).file_name().and_then(|n| n.to_str()).filter(|n| !n.is_empty()).unwrap_or("attachment");
    let ext = image_ext(mime)?;
    let known = base.rsplit_once('.').is_some_and(|(_, e)| matches!(e.to_ascii_lowercase().as_str(), "png" | "jpg" | "jpeg" | "gif" | "webp"));
    let file = if known { base.to_owned() } else { format!("{base}.{ext}") };
    std::fs::create_dir_all(dir).ok()?;
    let path = dir.join(file);
    std::fs::write(&path, bytes).ok()?;
    Some(path.display().to_string())
}

fn step(app: &mut App, p: &mut Pending) -> Outcome {
    let next = match &mut p.step {
        Step::Urls(urls) => {
            let Some(net) = app.src.net() else { return Outcome::Waiting };
            let base = net.base_url().trim_end_matches('/').to_owned();
            let mut moved = false;
            for (rpc, url) in urls.iter_mut().filter(|(_, url)| url.is_none()) {
                match net.take_reply(*rpc) {
                    None => {}
                    Some(Ok(reply)) => {
                        *url = Some(format!("{base}{}", s(&reply, "relativeUrl")));
                        moved = true;
                    }
                    Some(Err(e)) => return Outcome::Failed(failure(&e, FAILED)),
                }
            }
            if urls.iter().any(|(_, url)| url.is_none()) {
                return if moved { Outcome::Moved } else { Outcome::Waiting };
            }
            let jobs = urls
                .iter()
                .zip(&p.rewind.attachments)
                .map(|((_, url), a)| (url.clone().unwrap_or_default(), s(a, "name").to_owned(), s(a, "mimeType").to_owned()))
                .collect();
            Step::Download(download(jobs, app.ui.panel.waker.clone()))
        }
        Step::Download(rx) => match rx.try_recv() {
            Ok(Ok(files)) => {
                p.files = files;
                match roll_back(app, &p.rewind, p.restore_files) {
                    Ok(step) => step,
                    Err(message) => return Outcome::Failed(message),
                }
            }
            Ok(Err(message)) => return Outcome::Failed(message),
            Err(TryRecvError::Empty) => return Outcome::Waiting,
            Err(TryRecvError::Disconnected) => return Outcome::Failed(FAILED.into()),
        },
        Step::Rollback { rpc, watch, started } => {
            let mut moved = false;
            if let Some(id) = *rpc {
                let Some(net) = app.src.net() else { return Outcome::Waiting };
                match net.take_reply(id) {
                    None => {}
                    Some(Ok(_)) => {
                        *rpc = None;
                        moved = true;
                    }
                    Some(Err(e)) => return Outcome::Failed(failure(&e, FAILED)),
                }
            }
            let projection = app.state().thread_views.get(&p.rewind.thread).map(|v| &v.projection);
            return match projection.and_then(|projection| settled(projection, watch, rpc.is_none())) {
                Some(Ok(())) => Outcome::Done,
                Some(Err(message)) => Outcome::Failed(message),
                None if crate::data::now_ms() - *started >= TIMEOUT_MS => Outcome::Failed("Timed out waiting for the thread to rewind.".into()),
                None if moved => Outcome::Moved,
                None => Outcome::Waiting,
            };
        }
    };
    p.step = next;
    Outcome::Moved
}

/// The rewound prompt and its images go back into the thread's draft; the caret follows only
/// while that thread is still open.
fn land(app: &mut App, p: Pending) {
    let thread = p.rewind.thread;
    let key = crate::chat::composer::draft_key(app, &Route::Thread(thread.clone()));
    let focus = matches!(app.frame_route(), Route::Thread(open) if *open == thread);
    crate::chat::composer::restore_rewound(app, &key, &recallable(&p.rewind.text), &p.rewind.records, p.files, focus);
}

pub fn poll(app: &mut App) -> bool {
    let mut changed = false;
    let threads: Vec<String> = app.ui.chat.timeline.actions.rewinds.keys().cloned().collect();
    for thread in threads {
        let Some(mut pending) = app.ui.chat.timeline.actions.rewinds.remove(&thread) else { continue };
        match step(app, &mut pending) {
            Outcome::Waiting => {
                app.ui.chat.timeline.actions.rewinds.insert(thread, pending);
            }
            Outcome::Moved => {
                app.ui.chat.timeline.actions.rewinds.insert(thread, pending);
                changed = true;
            }
            Outcome::Failed(message) => {
                app.ui.chat.timeline.actions.errors.insert(thread, message);
                changed = true;
            }
            Outcome::Done => {
                land(app, pending);
                changed = true;
            }
        }
    }
    let Some(net) = app.src.net() else { return changed };
    let state = &mut app.ui.chat.timeline.actions;
    let now = crate::data::now_ms();
    state.forks.retain(|rpc, (source, target)| {
        let Some(reply) = net.take_reply(*rpc) else { return true };
        changed = true;
        match reply {
            Ok(_) => state.landing.push((std::mem::take(source), std::mem::take(target), now)),
            Err(e) => {
                state.errors.insert(source.clone(), failure(&e, "Failed to fork this response."));
            }
        }
        false
    });
    let mut opened = None;
    let landing = std::mem::take(&mut app.ui.chat.timeline.actions.landing);
    for (source, target, since) in landing {
        match fork_landed(&app.state().threads, &target, since, now) {
            None => app.ui.chat.timeline.actions.landing.push((source, target, since)),
            Some(Ok(())) => opened = Some(target),
            Some(Err(message)) => {
                app.ui.chat.timeline.actions.errors.insert(source, message.into());
                changed = true;
            }
        }
    }
    if let Some(target) = opened {
        crate::shell::sidebar::open_thread(app, &target);
        changed = true;
    }
    changed
}

/// waitForThreadShell: the fork opens once its thread is here, and gives up after five seconds.
fn fork_landed(threads: &[Value], target: &str, since: i64, now: i64) -> Option<Result<(), &'static str>> {
    if threads.iter().any(|t| s(t, "id") == target) {
        Some(Ok(()))
    } else if now - since >= FORK_SHELL_MS {
        Some(Err(FORK_LOST))
    } else {
        None
    }
}

/// "Edit from here?" (RevertConfirmHost), over its thread.
pub fn overlays(app: &App, t: &T) -> Vec<El> {
    let mut layers = Vec::new();
    let Route::Thread(thread) = app.frame_route() else { return layers };
    let Some((_, notice)) = app.ui.chat.timeline.actions.confirm.get(thread) else { return layers };
    let cancel = {
        let thread = thread.clone();
        move |app: &mut App| {
            app.ui.chat.timeline.actions.confirm.remove(&thread);
        }
    };
    let confirm = |restore_files: bool| {
        let thread = thread.clone();
        move |app: &mut App| rewind(app, &thread, restore_files)
    };
    let mut dialog = crate::kit::Dialog::new("revert-confirm", "Edit from here?")
        .open(true)
        .alert()
        .description("Rewind chat to before this message. Your prompt and attachments return to the composer. “Revert files too” also undoes your own edits.")
        .on_close(cancel.clone());
    if let Some(notice) = notice {
        dialog = dialog.child(text(notice.clone()).font(theme::TEXT_SM).color(t.c.destructive).wrap());
    }
    dialog
        .child(
            row()
                .w_full()
                .justify_end()
                .gap(8.0)
                .child(crate::kit::Button::new("Cancel").variant("outline").on_press(cancel).build(t))
                .child(crate::kit::Button::new("Revert files too").variant("destructive-outline").on_press(confirm(true)).build(t))
                .child(crate::kit::Button::new("Revert and keep changes").on_press(confirm(false)).build(t)),
        )
        .build(t, &mut layers);
    layers
}

/// The open thread's failed revert or fork, above the timeline's end (setThreadError).
pub fn error(app: &App, t: &T) -> Option<El> {
    let Route::Thread(thread) = app.frame_route() else { return None };
    let message = app.ui.chat.timeline.actions.errors.get(thread)?.clone();
    let thread = thread.clone();
    Some(
        row()
            .key("timeline-action-error")
            .w_full()
            .px(12.0)
            .py(8.0)
            .gap(8.0)
            .center_y()
            .radius(8.0)
            .border(1.0, t.c.destructive.alpha(0.3))
            .child(icon(&icons::CIRCLE_ALERT, 16.0).color(t.c.destructive))
            .child(text(message).font(theme::TEXT_SM).color(t.c.destructive).wrap().grow(1.0).shrink().min_w(0.0))
            .child(ghost(t, "timeline-action-error-close".into(), &icons::X, "Dismiss").on_press(move |app: &mut App| {
                app.ui.chat.timeline.actions.errors.remove(&thread);
            })),
    )
}

/// The prompt a rewind puts back (recallableComposerPrompt): without Claude's ultrathink prefix,
/// trailing review comments or legacy context blocks, and empty for a bootstrap or plan prompt.
/// Context chips stay, as their records come back with them.
pub fn recallable(text: &str) -> String {
    let text = text.trim();
    let mut prompt = text.strip_prefix("Ultrathink:\n").unwrap_or(text).to_owned();
    while !prompt.is_empty() {
        if let Some(cut) = review_comments_cut(&prompt) {
            prompt.truncate(cut);
            prompt.truncate(prompt.trim_end().len());
            continue;
        }
        let Some((at, tag, body)) = trailing_context(&prompt) else { break };
        let lines: Vec<&str> = body.split('\n').collect();
        if tag == "preview_annotation"
            && lines.iter().any(|l| l.trim_end() == "<preview_annotation>")
            && !lines.iter().any(|l| ["Id: ", "Page: ", "Comment: ", "Targets: "].iter().any(|k| l.starts_with(k)))
        {
            break;
        }
        let headers: Vec<String> =
            lines.iter().filter_map(|l| l.strip_prefix("- ")?.strip_suffix(':').filter(|h| !h.is_empty()).map(str::to_owned)).collect();
        if tag != "preview_annotation" && headers.is_empty() {
            break;
        }
        let kept = prompt[..at].trim_end();
        prompt = if tag == "terminal_context" { strip_terminal_labels(kept, &headers) } else { kept.to_owned() };
    }
    let trimmed = prompt.trim();
    if trimmed == BOOTSTRAP_PROMPT || trimmed.starts_with(super::plan::IMPLEMENTATION_PREFIX) {
        return String::new();
    }
    trimmed.to_owned()
}

/// Where trailing `<review_comment>` blocks (only whitespace between them and the end) start.
fn review_comments_cut(p: &str) -> Option<usize> {
    const OPEN: &str = "<review_comment";
    const CLOSE: &str = "</review_comment>";
    let mut blocks = Vec::new();
    let mut from = 0;
    while let Some(at) = p[from..].find(OPEN).map(|i| from + i) {
        let after = at + OPEN.len();
        let end = p[after..]
            .starts_with(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
            .then(|| p[after..].find('>'))
            .flatten()
            .and_then(|gt| p[after + gt + 1..].find(CLOSE).map(|close| after + gt + 1 + close + CLOSE.len()));
        match end {
            Some(end) => {
                blocks.push((at, end));
                from = end;
            }
            None => from = after,
        }
    }
    let mut cut = p.len();
    for &(start, end) in blocks.iter().rev() {
        if !p[end..cut].trim().is_empty() {
            break;
        }
        cut = start;
    }
    (cut < p.len()).then_some(cut)
}

/// A legacy context block closing the prompt: where it starts, its tag and its body.
fn trailing_context(p: &str) -> Option<(usize, &'static str, &str)> {
    let p = p.trim_end();
    ["terminal_context", "element_context", "preview_annotation"].into_iter().find_map(|tag| {
        let body_end = p.strip_suffix(format!("\n</{tag}>").as_str())?.len();
        let open = format!("<{tag}>\n");
        let at = p.find(open.as_str())?;
        let body_start = at + open.len();
        (body_start <= body_end).then(|| (at, tag, &p[body_start..body_end]))
    })
}

/// Drops each terminal header's inline `@terminal-1:3-5` mention and a space beside it.
fn strip_terminal_labels(prompt: &str, headers: &[String]) -> String {
    let mut out = prompt.to_owned();
    for header in headers {
        let Some((head, range)) = header.rsplit_once(' ') else { continue };
        let digits = |d: &str| !d.is_empty() && d.bytes().all(|b| b.is_ascii_digit());
        let range_ok = match range.split_once('-') {
            Some((a, b)) => digits(a) && digits(b),
            None => digits(range),
        };
        let Some(name) = head.strip_suffix(" lines").or_else(|| head.strip_suffix(" line")).filter(|n| !n.is_empty()) else { continue };
        if !range_ok {
            continue;
        }
        let label = format!("@{}:{range}", name.trim().to_lowercase().split_whitespace().collect::<Vec<_>>().join("-"));
        let found = out.match_indices(label.as_str()).map(|(i, _)| i).find(|&i| {
            let before = out[..i].chars().next_back();
            let after = out[i + label.len()..].chars().next();
            !before.is_some_and(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '@' | '.' | '-'))
                && !after.is_some_and(|c| c.is_ascii_digit() || c == '-')
        });
        let Some(i) = found else { continue };
        let (mut start, mut end) = (i, i + label.len());
        if out[end..].starts_with(' ') {
            end += 1;
        } else if out[..start].ends_with(' ') {
            start -= 1;
        }
        out.replace_range(start..end, "");
    }
    out
}

/// Whether a settled run's changes are big enough to open the diff on their own
/// (resolveProactiveTurnDiffAction): a ready checkpoint with 3 or more files or 50 or more changed
/// lines. False too while its checkpoint hasn't arrived, which the web defers on.
pub fn proactive_changes(projection: &Value, run_id: &str) -> bool {
    let Some(checkpoint) = rows::array(projection, "checkpoints").iter().rfind(|c| s(c, "runId") == run_id && !c["appRunOrdinal"].is_null()) else {
        return false;
    };
    let files = rows::array(checkpoint, "files");
    if s(checkpoint, "status") != "ready" || files.is_empty() {
        return false;
    }
    let lines = |v: &Value| v["additions"].as_u64().unwrap_or(0) + v["deletions"].as_u64().unwrap_or(0);
    let totals = &checkpoint["filesTotal"];
    let (count, changed) = if totals.is_object() {
        (totals["count"].as_u64().unwrap_or(0), lines(totals))
    } else {
        (files.len() as u64, files.iter().map(lines).sum())
    };
    count >= 3 || changed >= 50
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_fork_opens_once_its_thread_lands_or_reports_it_never_did() {
        let threads = [json!({"id": "source"}), json!({"id": "fork"})];
        assert_eq!(fork_landed(&threads, "fork", 0, 100), Some(Ok(())));
        assert_eq!(fork_landed(&threads[..1], "fork", 0, FORK_SHELL_MS - 1), None);
        assert_eq!(fork_landed(&threads[..1], "fork", 0, FORK_SHELL_MS), Some(Err(FORK_LOST)));
    }

    #[test]
    fn timestamps_name_the_day_only_when_it_is_not_today() {
        let now = iso_ms("2026-11-08T15:00:00.000Z").unwrap();
        let utc = Zone::Fixed(0);
        assert_eq!(timestamp("2026-11-08T09:05:00.000Z", now, utc, true), "9:05 AM");
        assert_eq!(timestamp("2026-11-07T00:30:00.000Z", now, utc, true), "yesterday at 12:30 AM");
        assert_eq!(timestamp("2026-03-14T13:41:00.000Z", now, utc, true), "3/14 1:41 PM");
        assert_eq!(timestamp("2025-12-31T23:59:00.000Z", now, utc, true), "12/31/25 11:59 PM");
        assert_eq!(timestamp("2026-11-08T09:05:00.000Z", now, utc, false), "09:05");
        // Days turn at local midnight: 00:30 UTC is still yesterday evening in New York.
        let new_york = Zone::Fixed(-5 * 3_600_000);
        assert_eq!(timestamp("2026-11-08T00:30:00.000Z", now, new_york, true), "yesterday at 7:30 PM");
        assert_eq!(full_timestamp("2026-03-03T09:41:00.000Z", utc, true), "9:41 AM, 3rd March 2026");
        assert_eq!(full_timestamp("2026-03-11T21:41:00.000Z", utc, false), "21:41, 11th March 2026");
        assert_eq!(upcoming("2026-11-08T17:16:00.000Z", now, utc, true), "5:16 PM");
        assert_eq!(upcoming("2026-11-09T14:16:00.000Z", now, utc, true), "tomorrow at 2:16 PM");
        assert_eq!(upcoming("2026-11-12T14:16:00.000Z", now, utc, true), "11/12 2:16 PM");
    }

    #[test]
    fn a_rollback_settles_once_the_provider_and_the_witness_run_rewind() {
        let before = json!({
            "thread": {"id": "t", "activeProviderThreadId": "pt"},
            "providerThreads": [{"id": "pt", "providerInstanceId": "codex", "lastRunOrdinal": 2}],
            "runs": [
                {"id": "r1", "ordinal": 1, "status": "completed"},
                {"id": "r2", "ordinal": 2, "status": "interrupted"},
                {"id": "r3", "ordinal": 3, "status": "running"},
            ],
        });
        let w = watch(&before, "cmd", 1).unwrap();
        assert_eq!(w.witness, "r2");
        assert!(watch(&before, "cmd", 2).is_none(), "nothing settled is left after the last settled run");
        assert_eq!(settled(&before, &w, true), None);
        let mut after = before.clone();
        after["providerThreads"][0]["lastRunOrdinal"] = json!(1);
        after["runs"][1]["status"] = json!("rolled_back");
        assert_eq!(settled(&after, &w, false), None, "not before the server accepts it");
        assert_eq!(settled(&after, &w, true), Some(Ok(())));
        let mut failed = before.clone();
        failed["thread"]["rollbackFailure"] = json!({"requestId": "cmd", "message": "Checkpoint is gone."});
        assert_eq!(settled(&failed, &w, false), Some(Err("Checkpoint is gone.".to_owned())));
        failed["thread"]["rollbackFailure"]["requestId"] = json!("other");
        assert_eq!(settled(&failed, &w, true), None);
        // Claude rewinds to the start on a fresh root provider thread.
        let w = watch(&before, "cmd", 0).unwrap();
        let mut fresh = before.clone();
        fresh["providerThreads"] = json!([
            {"id": "pt", "providerInstanceId": "codex", "lastRunOrdinal": 2},
            {"id": "pt2", "providerInstanceId": "codex", "appThreadId": "t", "ownerNodeId": null, "lastRunOrdinal": null},
        ]);
        fresh["runs"][1]["status"] = json!("rolled_back");
        assert_eq!(settled(&fresh, &w, true), Some(Ok(())));
    }

    #[test]
    fn a_recalled_prompt_drops_ultrathink_review_comments_and_legacy_context() {
        assert_eq!(recallable("Ultrathink:\nFix it"), "Fix it");
        assert_eq!(recallable("Fix it\n\n<review_comment file=\"a.rs\">nit</review_comment>\n<review_comment>x</review_comment>\n"), "Fix it");
        assert_eq!(recallable("<review_comment>x</review_comment> then this"), "<review_comment>x</review_comment> then this");
        let terminal = "Look @terminal-1:3-5 here\n\n<terminal_context>\n- Terminal 1 lines 3-5:\n  ls\n</terminal_context>";
        assert_eq!(recallable(terminal), "Look here");
        let headless = "Hi\n<terminal_context>\nplain\n</terminal_context>";
        assert_eq!(recallable(headless), headless);
        assert_eq!(recallable("PLEASE IMPLEMENT THIS PLAN:\n# Plan"), "");
        assert_eq!(recallable(BOOTSTRAP_PROMPT), "");
    }

    #[test]
    fn big_changes_open_the_diff_on_their_own() {
        let p = |files: Value, total: Value, status: &str| json!({"checkpoints": [{"runId": "r", "appRunOrdinal": 1, "status": status, "files": files, "filesTotal": total}]});
        let three = json!([{"path": "a"}, {"path": "b"}, {"path": "c"}]);
        assert!(proactive_changes(&p(three.clone(), Value::Null, "ready"), "r"));
        assert!(!proactive_changes(&p(three, Value::Null, "missing"), "r"));
        let one = json!([{"path": "a", "additions": 30, "deletions": 19}]);
        assert!(!proactive_changes(&p(one.clone(), Value::Null, "ready"), "r"));
        assert!(proactive_changes(&p(one, json!({"count": 1, "additions": 40, "deletions": 10}), "ready"), "r"));
        assert!(!proactive_changes(&p(json!([]), json!({"count": 9}), "ready"), "r"));
        assert!(!proactive_changes(&json!({}), "r"));
    }
}
