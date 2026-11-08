//! The sidebar's pure rules, ported from apps/web `Sidebar.logic.ts` and client-runtime's
//! `models.ts`, `threadSort.ts`, `threadInbox.ts` and `threadSettled.ts`. Tests mirror theirs.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};

use serde_json::Value;

use crate::data::iso_ms;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Run {
    pub id: String,
    pub status: String,
    pub requested_at: Option<i64>,
    pub started_at: Option<i64>,
    pub completed_at: Option<i64>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Runtime {
    pub status: String,
    pub active_run_id: Option<String>,
    /// `None` when the server sent no field; `Some(None)` when it sent null.
    pub activity_started_at: Option<Option<i64>>,
    pub last_error_class: Option<String>,
    pub updated_at: Option<i64>,
}

/// One sidebar row's inputs, read from a shell thread (`presentThreadShell`).
#[derive(Clone, Debug, Default)]
pub struct Thread {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub provider: String,
    pub branch: Option<String>,
    pub worktree: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
    pub latest_user_message_at: Option<i64>,
    /// `None` when the field is absent (older servers), as `=== undefined` in the web.
    pub latest_user_authored_at: Option<Option<i64>>,
    pub run: Option<Run>,
    pub runtime: Option<Runtime>,
    pub blocked_on_wait: bool,
    pub pending_approval: bool,
    pub pending_input: bool,
    pub pending_request: bool,
    pub guests: u32,
    pub plan_mode: bool,
    pub actionable_plan: bool,
    pub goal_active: bool,
    pub archived: bool,
    /// Subagent and side-chat threads never get their own row.
    pub hidden: bool,
    pub settled: bool,
    pub settled_at: Option<i64>,
    pub unsettled_at: Option<i64>,
    pub snoozed_until: Option<i64>,
    pub snoozed_at: Option<i64>,
    pub pinned: bool,
    pub pin_key: Option<String>,
    pub active_key: Option<String>,
    /// `Some(i64::MIN)` for an unparseable visit, which reads as "long ago".
    pub last_visited_at: Option<i64>,
}

fn opt_ts(v: &Value) -> Option<i64> {
    v.as_str().and_then(iso_ms)
}

fn opt_str(v: &Value) -> Option<String> {
    v.as_str().filter(|s| !s.is_empty()).map(str::to_string)
}

const TERMINAL: [&str; 5] = ["completed", "interrupted", "failed", "cancelled", "rolled_back"];
const ACTIVE: [&str; 5] = ["preparing", "queued", "starting", "running", "waiting"];

pub fn runtime_active(status: &str) -> bool {
    ACTIVE.contains(&status)
}

impl Thread {
    pub fn from_shell(v: &Value) -> Thread {
        let status = v["status"].as_str().unwrap_or("idle");
        let updated_at = opt_ts(&v["updatedAt"]).unwrap_or(0);
        let run = v["latestRunId"].as_str().map(|id| Run {
            id: id.to_string(),
            status: if status == "idle" { "completed".into() } else { status.into() },
            requested_at: opt_ts(&v["latestRunRequestedAt"]),
            started_at: opt_ts(&v["latestRunStartedAt"]),
            completed_at: match v.get("latestRunCompletedAt") {
                None => (status == "idle" || TERMINAL.contains(&status)).then_some(updated_at),
                Some(c) => opt_ts(c),
            },
        });
        let tasks = v["pendingBackgroundTasks"].as_array().map(Vec::as_slice).unwrap_or(&[]);
        let holds = tasks.iter().any(|t| t["awaited"] == true || t["kind"] == "subagent" || t["kind"] == "background_task");
        let park = holds && status != "failed";
        let runtime = if v["latestRunId"].is_null() && v["activeProviderThreadId"].is_null() && !park {
            None
        } else {
            Some(Runtime {
                status: if park { "idle".into() } else { v["activityRunStatus"].as_str().unwrap_or(status).into() },
                active_run_id: opt_str(&v["activeRunId"]),
                activity_started_at: v.get("activityRunStartedAt").map(opt_ts),
                last_error_class: opt_str(&v["lastErrorClass"]),
                updated_at: Some(updated_at),
            })
        };
        let request = v["pendingRuntimeRequest"]["kind"].as_str();
        let rel = v["lineage"]["relationshipToParent"].as_str();
        Thread {
            id: v["id"].as_str().unwrap_or_default().into(),
            project_id: v["projectId"].as_str().unwrap_or_default().into(),
            title: v["title"].as_str().unwrap_or_default().into(),
            provider: v["providerInstanceId"].as_str().or(v["modelSelection"]["instanceId"].as_str()).unwrap_or_default().into(),
            branch: opt_str(&v["branch"]),
            worktree: opt_str(&v["worktreePath"]),
            created_at: opt_ts(&v["createdAt"]).unwrap_or(0),
            updated_at,
            latest_user_message_at: opt_ts(&v["latestUserMessageAt"]),
            latest_user_authored_at: v.get("latestUserAuthoredMessageAt").map(opt_ts),
            run,
            runtime,
            blocked_on_wait: v["blockedOnThreadWait"] == true,
            pending_approval: request.is_some_and(|k| k != "user_input" && k != "auth_refresh"),
            pending_input: request == Some("user_input"),
            pending_request: request.is_some(),
            guests: v["pendingGuestMessageCount"].as_u64().unwrap_or(0) as u32,
            plan_mode: v["interactionMode"] == "plan",
            actionable_plan: v["hasActionableProposedPlan"] == true,
            goal_active: v["goal"]["status"] == "active",
            archived: !v["archivedAt"].is_null() || !v["deletedAt"].is_null(),
            hidden: rel == Some("subagent") || rel == Some("sideChat"),
            settled: v["settledOverride"] == "settled",
            settled_at: opt_ts(&v["settledAt"]),
            unsettled_at: opt_ts(&v["unsettledAt"]),
            snoozed_until: opt_ts(&v["snoozedUntil"]),
            snoozed_at: opt_ts(&v["snoozedAt"]),
            pinned: !v["pinnedAt"].is_null(),
            pin_key: opt_str(&v["pinOrderKey"]),
            active_key: opt_str(&v["activeOrderKey"]),
            last_visited_at: v["lastVisitedAt"].as_str().map(|s| iso_ms(s).unwrap_or(i64::MIN)),
        }
    }

    fn runtime_status(&self) -> Option<&str> {
        self.runtime.as_ref().map(|r| r.status.as_str())
    }

    pub fn running(&self) -> bool {
        self.runtime_status().is_some_and(runtime_active)
    }

    /// `threadRaisedHandWhileSnoozed`.
    fn raised_hand(&self) -> bool {
        if self.pending_approval || self.pending_input || self.pending_request || self.guests > 0 {
            return true;
        }
        match self.failure_edge() {
            None => false,
            Some(failed_at) => self.snoozed_at.is_none() || failed_at.zip(self.snoozed_at).is_some_and(|(f, s)| f > s),
        }
    }

    fn failure_edge(&self) -> Option<Option<i64>> {
        let rt = self.runtime.as_ref()?;
        if rt.status != "failed" && rt.status != "error" {
            return None;
        }
        Some(match &self.run {
            Some(run) if run.status == "failed" => run.completed_at,
            _ => rt.updated_at,
        })
    }

    /// `effectiveSnoozed`.
    pub fn snoozed(&self, now: i64) -> bool {
        self.snoozed_until.is_some_and(|w| w > now) && !self.raised_hand()
    }

    /// `threadWokeAt`.
    pub fn woke_at(&self, now: i64) -> Option<i64> {
        let wake = self.snoozed_until?;
        if self.raised_hand() {
            if let Some(Some(f)) = self.failure_edge()
                && self.snoozed_at.is_none_or(|s| f > s)
            {
                return Some(f);
            }
            return self.runtime.as_ref().and_then(|r| r.updated_at).or(self.snoozed_at);
        }
        (wake <= now).then_some(wake)
    }

    /// `isThreadWoke`.
    pub fn woke(&self, now: i64) -> bool {
        let Some(woke) = self.woke_at(now) else { return false };
        if self.settled {
            return false;
        }
        [self.last_visited_at.filter(|v| *v != i64::MIN), self.run.as_ref().and_then(|r| r.completed_at)].iter().flatten().all(|ms| *ms < woke)
    }

    /// `hasUnseenCompletion`.
    pub fn unread(&self) -> bool {
        let Some(done) = self.run.as_ref().and_then(|r| r.completed_at) else { return false };
        match self.last_visited_at {
            None => false,
            Some(seen) => done > seen,
        }
    }

    /// `canSnooze`, without the queued-turn check (shell rows carry no queue).
    pub fn can_snooze(&self) -> bool {
        !(self.pending_approval || self.pending_input || self.pending_request || self.guests > 0)
    }

    /// `isThreadWorking`.
    pub fn working(&self) -> bool {
        if self.pending_approval || self.pending_input {
            return false;
        }
        let Some(rt) = &self.runtime else { return false };
        if !runtime_active(&rt.status) && rt.status != "idle" {
            return false;
        }
        let run_settled = self.run.as_ref().is_some_and(|r| !runtime_active(&r.status) && rt.active_run_id.as_deref() != Some(r.id.as_str()));
        !(self.plan_mode && self.actionable_plan && run_settled)
    }

    /// `resolveThreadWorkingStartedAt`.
    pub fn working_started_at(&self) -> Option<i64> {
        if let Some(at) = self.runtime.as_ref().and_then(|r| r.activity_started_at) {
            return at;
        }
        let run = self.run.as_ref()?;
        if run.completed_at.is_none() && self.runtime.as_ref().and_then(|r| r.active_run_id.as_deref()) == Some(run.id.as_str()) {
            return run.started_at.or(run.requested_at);
        }
        None
    }

    /// The card's timestamp: `latestUserMessageAt ?? updatedAt`.
    pub fn card_time(&self) -> i64 {
        self.latest_user_message_at.unwrap_or(self.updated_at)
    }

    /// `resolveSettledThreadTimestamp`.
    pub fn settled_time(&self) -> i64 {
        if let Some(at) = self.settled_at {
            return at;
        }
        let run = self.run.as_ref();
        [self.latest_user_message_at, run.and_then(|r| r.requested_at), run.and_then(|r| r.started_at), run.and_then(|r| r.completed_at)]
            .into_iter()
            .flatten()
            .max()
            .unwrap_or(self.updated_at)
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Approval,
    Input,
    Request,
    Waiting,
    Working,
    Limited,
    Failed,
    Ready,
}

/// `resolveSidebarThreadStatus`.
pub fn status(t: &Thread) -> Status {
    if t.pending_approval {
        return Status::Approval;
    }
    if t.pending_input {
        return Status::Input;
    }
    if t.guests > 0 {
        return Status::Request;
    }
    let Some(rt) = &t.runtime else { return Status::Ready };
    if rt.status == "running" && t.blocked_on_wait {
        return Status::Waiting;
    }
    if runtime_active(&rt.status) {
        return Status::Working;
    }
    match rt.status.as_str() {
        "idle" => Status::Waiting,
        "failed" if rt.last_error_class.as_deref() == Some("usage_limit") => Status::Limited,
        "failed" => Status::Failed,
        _ => Status::Ready,
    }
}

/// `shouldRecedeSidebarThread`.
pub fn recedes(status: Status, unread: bool, woke: bool, active: bool, selected: bool) -> bool {
    if active || selected {
        return false;
    }
    match status {
        Status::Input | Status::Request => false,
        Status::Working | Status::Waiting => true,
        Status::Ready | Status::Approval => !unread && !woke,
        _ => false,
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Section {
    Pinned,
    Active,
    Working,
    Snoozed,
    Settled,
}

pub fn section(t: &Thread, now: i64, working_shelf: bool) -> Section {
    if t.snoozed(now) {
        Section::Snoozed
    } else if t.settled {
        Section::Settled
    } else if t.pinned {
        Section::Pinned
    } else if working_shelf && t.working() {
        Section::Working
    } else {
        Section::Active
    }
}

/// Each section's rows, as indices into the thread slice, in display order.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Sections {
    pub pinned: Vec<usize>,
    pub active: Vec<usize>,
    pub working: Vec<usize>,
    pub snoozed: Vec<usize>,
    pub settled: Vec<usize>,
}

impl Sections {
    pub fn of(&self, s: Section) -> &[usize] {
        match s {
            Section::Pinned => &self.pinned,
            Section::Active => &self.active,
            Section::Working => &self.working,
            Section::Snoozed => &self.snoozed,
            Section::Settled => &self.settled,
        }
    }
}

pub fn partition(threads: &[Thread], now: i64, working_shelf: bool) -> Sections {
    let mut s = Sections::default();
    for (i, t) in threads.iter().enumerate() {
        if t.archived || t.hidden {
            continue;
        }
        match section(t, now, working_shelf) {
            Section::Pinned => s.pinned.push(i),
            Section::Active => s.active.push(i),
            Section::Working => s.working.push(i),
            Section::Snoozed => s.snoozed.push(i),
            Section::Settled => s.settled.push(i),
        }
    }
    sort_pinned(threads, &mut s.pinned);
    if working_shelf {
        sort_newest(threads, &mut s.active, inbox_return);
    } else {
        sort_active(threads, &mut s.active);
    }
    sort_newest(threads, &mut s.working, working_send);
    s.snoozed.sort_by(|a, b| threads[*a].snoozed_until.cmp(&threads[*b].snoozed_until).then_with(|| threads[*a].id.cmp(&threads[*b].id)));
    s.settled.sort_by(|a, b| threads[*b].settled_time().cmp(&threads[*a].settled_time()).then_with(|| threads[*a].id.cmp(&threads[*b].id)));
    s
}

/// `sortPinnedThreadsByOrderKey`.
pub fn sort_pinned(threads: &[Thread], rows: &mut [usize]) {
    rows.sort_by(|a, b| {
        let (l, r) = (&threads[*a], &threads[*b]);
        match (&l.pin_key, &r.pin_key) {
            (Some(lk), Some(rk)) => lk.cmp(rk).then_with(|| l.id.cmp(&r.id)),
            (Some(_), None) => Ordering::Less,
            (None, Some(_)) => Ordering::Greater,
            (None, None) => r.created_at.cmp(&l.created_at).then_with(|| l.id.cmp(&r.id)),
        }
    });
}

/// `sortActiveThreadsByOrderKey`: keyless rows (newest anchor first) above keyed rows.
pub fn sort_active(threads: &[Thread], rows: &mut [usize]) {
    let anchor = |t: &Thread| t.created_at.max(t.unsettled_at.unwrap_or(0));
    rows.sort_by(|a, b| {
        let (l, r) = (&threads[*a], &threads[*b]);
        let order = match (&l.active_key, &r.active_key) {
            (None, Some(_)) => Ordering::Less,
            (Some(_), None) => Ordering::Greater,
            (Some(lk), Some(rk)) => lk.cmp(rk),
            (None, None) => anchor(r).cmp(&anchor(l)),
        };
        order.then_with(|| l.id.cmp(&r.id))
    });
}

fn inbox_return(t: &Thread) -> i64 {
    let run = t.run.as_ref();
    [Some(t.created_at), t.unsettled_at, run.and_then(|r| r.requested_at), run.and_then(|r| r.completed_at)].into_iter().flatten().fold(0, i64::max)
}

fn working_send(t: &Thread) -> i64 {
    let sent = match t.latest_user_authored_at {
        None => t.run.as_ref().and_then(|r| r.requested_at),
        Some(at) => at,
    };
    t.created_at.max(sent.unwrap_or(0))
}

fn sort_newest(threads: &[Thread], rows: &mut [usize], at: fn(&Thread) -> i64) {
    rows.sort_by(|a, b| at(&threads[*b]).cmp(&at(&threads[*a])).then_with(|| threads[*a].id.cmp(&threads[*b].id)));
}

// --- Fractional order keys -------------------------------------------------------------------

const DIGITS: &[u8; 26] = b"abcdefghijklmnopqrstuvwxyz";

fn valid_key(k: &str) -> bool {
    !k.is_empty() && k.bytes().all(|b| b.is_ascii_lowercase()) && !k.ends_with('a')
}

fn digit(b: Option<u8>) -> usize {
    b.map_or(0, |b| (b - b'a') as usize)
}

/// Midpoint of two digit strings read as fractions in (0, 1); "" is the open bound.
fn midpoint(a: &str, b: &str) -> String {
    let (ab, bb) = (a.as_bytes(), b.as_bytes());
    if !b.is_empty() {
        let mut n = 0;
        while n < bb.len() && ab.get(n).copied().unwrap_or(b'a') == bb[n] {
            n += 1;
        }
        if n > 0 {
            return format!("{}{}", &b[..n], midpoint(a.get(n..).unwrap_or(""), &b[n..]));
        }
    }
    let da = if a.is_empty() { 0 } else { digit(ab.first().copied()) };
    let db = if b.is_empty() { 26 } else { digit(bb.first().copied()) };
    if db - da > 1 {
        // JS Math.round: halves round up.
        return (DIGITS[(da + db).div_ceil(2)] as char).to_string();
    }
    if b.len() > 1 {
        return b[..1].to_string();
    }
    format!("{}{}", DIGITS[da] as char, midpoint(a.get(1..).unwrap_or(""), ""))
}

/// `pinOrderKeyBetween`: a key strictly between two neighbours, `None` on corrupt bounds.
pub fn key_between(before: Option<&str>, after: Option<&str>) -> Option<String> {
    let (a, b) = (before.unwrap_or(""), after.unwrap_or(""));
    if (!a.is_empty() && !valid_key(a)) || (!b.is_empty() && !valid_key(b)) || (!b.is_empty() && a >= b) {
        return None;
    }
    Some(midpoint(a, b))
}

/// `generateSpreadPinOrderKeys`.
pub fn spread_keys(count: usize) -> Vec<String> {
    let (mut width, mut space) = (2u32, 26f64 * 26.0);
    while space <= ((count + 1) * 2) as f64 {
        width += 1;
        space *= 26.0;
    }
    let step = space / (count + 1) as f64;
    (0..count)
        .map(|i| {
            let mut value = (step * (i + 1) as f64 + 0.5).floor() as u64;
            if value % 26 == 0 {
                value += 1;
            }
            let mut key = vec![b'a'; width as usize];
            for d in (0..width as usize).rev() {
                key[d] = DIGITS[(value % 26) as usize];
                value /= 26;
            }
            String::from_utf8(key).unwrap()
        })
        .collect()
}

pub type Keys = HashMap<String, Option<String>>;
pub type Assignments = Vec<(String, String)>;

/// `planPinnedReorder`: the fewest key writes that put `moved` where `order` has it.
pub fn plan_reorder(order: &[String], keys: &Keys, moved: &str) -> Assignments {
    let visible: HashSet<&str> = order.iter().map(String::as_str).collect();
    let reserved: HashSet<&str> = keys.iter().filter(|(id, _)| !visible.contains(id.as_str())).filter_map(|(_, k)| k.as_deref()).collect();
    let Some(at) = order.iter().position(|id| id == moved) else { return Vec::new() };
    let key_of = |id: &String| keys.get(id).cloned().flatten();
    let before = at.checked_sub(1).map(|i| &order[i]);
    let after = order.get(at + 1);
    let (bk, ak) = (before.and_then(key_of), after.and_then(key_of));
    if (before.is_none() || bk.is_some()) && (after.is_none() || ak.is_some()) {
        let mut key = key_between(bk.as_deref(), ak.as_deref());
        while let Some(k) = key.as_deref().filter(|k| reserved.contains(k)) {
            key = key_between(Some(k), ak.as_deref());
        }
        if let Some(k) = key {
            return vec![(moved.to_string(), k)];
        }
    }
    let spread: Vec<String> = spread_keys(order.len() + reserved.len()).into_iter().filter(|k| !reserved.contains(k.as_str())).take(order.len()).collect();
    order.iter().zip(spread).filter(|(id, k)| key_of(id).as_deref() != Some(k.as_str())).map(|(id, k)| (id.clone(), k)).collect()
}

// --- Drag and drop ---------------------------------------------------------------------------

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marker {
    PinnedHeader,
    PinnedDivider,
    ActivePlaceholder,
    WorkingHeader,
    SnoozedHeader,
    SettledHeader,
    SettledPlaceholder,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Item {
    Thread { key: String, section: Section },
    Marker(Marker),
}

impl Item {
    pub fn id(&self) -> String {
        match self {
            Item::Thread { key, .. } => key.clone(),
            Item::Marker(m) => format!("sidebar-marker-{}", m.name()),
        }
    }
}

impl Marker {
    pub fn name(self) -> &'static str {
        match self {
            Marker::PinnedHeader => "pinned-header",
            Marker::PinnedDivider => "pinned-divider",
            Marker::ActivePlaceholder => "active-placeholder",
            Marker::WorkingHeader => "working-header",
            Marker::SnoozedHeader => "snoozed-header",
            Marker::SettledHeader => "settled-header",
            Marker::SettledPlaceholder => "settled-placeholder",
        }
    }
}

fn section_after(marker: Marker, current: Section) -> Section {
    match marker {
        Marker::PinnedDivider => Section::Active,
        Marker::WorkingHeader => Section::Working,
        Marker::SnoozedHeader => Section::Snoozed,
        Marker::SettledHeader => Section::Settled,
        _ => current,
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct DropTarget {
    pub section: Section,
    pub pinned_order: Vec<String>,
    pub active_order: Vec<String>,
}

/// `resolveSidebarDropTarget`: where dropping `active` over `over` lands.
pub fn drop_target(items: &[Item], active: &str, over: &str) -> Option<DropTarget> {
    let ai = items.iter().position(|i| i.id() == active)?;
    let oi = items.iter().position(|i| i.id() == over)?;
    if !matches!(items[ai], Item::Thread { .. }) {
        return None;
    }
    let mut moved: Vec<&Item> = items.iter().enumerate().filter(|(i, _)| *i != ai).map(|(_, it)| it).collect();
    moved.insert(oi.min(moved.len()), &items[ai]);
    let section = moved[..oi].iter().fold(Section::Pinned, |s, it| if let Item::Marker(m) = it { section_after(*m, s) } else { s });
    if matches!(section, Section::Working | Section::Snoozed) {
        return None;
    }
    let (mut pinned_order, mut active_order, mut current) = (Vec::new(), Vec::new(), Section::Pinned);
    for it in moved {
        match it {
            Item::Marker(Marker::PinnedDivider) => current = Section::Active,
            Item::Marker(Marker::WorkingHeader | Marker::SnoozedHeader | Marker::SettledHeader) => break,
            Item::Marker(_) => {}
            Item::Thread { key, .. } if current == Section::Pinned => pinned_order.push(key.clone()),
            Item::Thread { key, .. } => active_order.push(key.clone()),
        }
    }
    Some(DropTarget { section, pinned_order, active_order })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verb {
    Pin,
    Unpin,
    Settle,
    Unsettle,
    Wake,
}

impl Verb {
    pub fn label(self) -> &'static str {
        match self {
            Verb::Pin => "Pin",
            Verb::Unpin => "Unpin",
            Verb::Settle => "Settle",
            Verb::Unsettle => "Un-settle",
            Verb::Wake => "Wake",
        }
    }
}

/// `resolveSidebarDropVerb`.
pub fn drop_verb(from: Section, to: Option<Section>) -> Option<Verb> {
    let to = to?;
    if to == from || matches!(to, Section::Working | Section::Snoozed) {
        return None;
    }
    Some(match (from, to) {
        (_, Section::Pinned) => Verb::Pin,
        (_, Section::Settled) => Verb::Settle,
        (Section::Pinned, _) => Verb::Unpin,
        (Section::Settled, _) => Verb::Unsettle,
        _ => Verb::Wake,
    })
}

#[derive(Clone, Debug, PartialEq)]
pub enum Plan {
    None,
    ReorderPinned { order: Vec<String>, assignments: Assignments },
    Pin { order: Vec<String>, key: Option<String>, extra: Assignments },
    MoveActive { order: Option<Vec<String>>, assignments: Assignments, unpin: bool, unsettle: bool, unsnooze: bool },
    Settle,
}

pub struct DropInput<'a> {
    pub active: &'a str,
    pub section: Section,
    pub target: &'a DropTarget,
    pub pinned_order: &'a [String],
    pub pinned_keys: &'a Keys,
    pub pinned_writable: Option<&'a HashSet<String>>,
    pub active_order: &'a [String],
    pub active_keys: &'a Keys,
    pub active_writable: Option<&'a HashSet<String>>,
    pub time_ordered: bool,
}

/// `planSidebarThreadDrop`.
pub fn plan_drop(i: &DropInput) -> Plan {
    let (active, section, target) = (i.active, i.section, i.target);
    let (pinned, settled) = (section == Section::Pinned, section == Section::Settled);
    if section == Section::Working {
        return Plan::None;
    }
    let arrange = |order: &[String], keys: &Keys, writable: Option<&HashSet<String>>| -> Option<Assignments> {
        let Some(writable) = writable else { return Some(plan_reorder(order, keys, active)) };
        if !writable.contains(active) {
            return None;
        }
        let has_key = |k: &String| keys.get(k).is_some_and(Option::is_some);
        let kept: Vec<String> = order.iter().filter(|k| writable.contains(*k) || has_key(k)).cloned().collect();
        let assignments = plan_reorder(&kept, keys, active);
        if assignments.iter().any(|(id, _)| !writable.contains(id)) {
            return None;
        }
        // Keyless Active rows sort before keyed rows; keyless Pinned rows after them.
        let assigned: HashSet<&str> = assignments.iter().map(|(id, _)| id.as_str()).collect();
        let (mut frozen_before, mut assigned_before) = (false, false);
        for id in order {
            let frozen = !writable.contains(id) && !has_key(id);
            let is_assigned = assigned.contains(id.as_str());
            if (target.section == Section::Active && frozen && assigned_before) || (target.section == Section::Pinned && is_assigned && frozen_before) {
                return None;
            }
            frozen_before |= frozen;
            assigned_before |= is_assigned;
        }
        Some(assignments)
    };
    match target.section {
        Section::Active => {
            let leave = |order, assignments| Plan::MoveActive { order, assignments, unpin: pinned, unsettle: settled, unsnooze: section == Section::Snoozed };
            if i.time_ordered {
                return if section == Section::Active { Plan::None } else { leave(None, Vec::new()) };
            }
            let order = &target.active_order;
            if section == Section::Active && order.as_slice() == i.active_order {
                return Plan::None;
            }
            match arrange(order, i.active_keys, i.active_writable) {
                None => Plan::None,
                Some(a) => leave(Some(order.clone()), a),
            }
        }
        Section::Settled => {
            if settled {
                Plan::None
            } else {
                Plan::Settle
            }
        }
        Section::Pinned => {
            let order = &target.pinned_order;
            if pinned && order.as_slice() == i.pinned_order {
                return Plan::None;
            }
            let Some(assignments) = arrange(order, i.pinned_keys, i.pinned_writable) else { return Plan::None };
            if pinned {
                return if assignments.is_empty() { Plan::None } else { Plan::ReorderPinned { order: order.clone(), assignments } };
            }
            let key = assignments.iter().find(|(id, _)| id == active).map(|(_, k)| k.clone());
            let extra = assignments.into_iter().filter(|(id, _)| id != active).collect();
            Plan::Pin { order: order.clone(), key, extra }
        }
        Section::Working | Section::Snoozed => Plan::None,
    }
}

// --- Labels ---------------------------------------------------------------------------------

/// The card's relative time: `formatRelativeElapsed` floored to its unit, " ago" dropped.
pub fn relative_label(ts: i64, now: i64) -> String {
    let elapsed = (now - ts).max(0);
    let s = elapsed / 1000;
    let (m, h) = (s / 60, s / 3600);
    if s < 60 {
        "now".into()
    } else if m < 60 {
        format!("{m}m")
    } else if h < 24 {
        format!("{h}h")
    } else {
        format!("{}d", h / 24)
    }
}

/// `formatWorkingDurationLabel`.
pub fn duration_label(elapsed_ms: i64) -> String {
    let s = elapsed_ms.max(0) / 1000;
    let m = s / 60;
    if s < 60 {
        format!("{s}s")
    } else if m < 60 {
        format!("{m}m")
    } else {
        format!("{}h {}m", m / 60, m % 60)
    }
}

/// `projectMonogram`: two glyphs from a project's name.
pub fn monogram(name: &str) -> String {
    let words: Vec<Vec<char>> = name
        .trim()
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(|w| w.chars().collect())
        .collect();
    let Some(first_word) = words.first() else { return "PR".into() };
    let first = first_word[0];
    let second = first_word[1..]
        .iter()
        .copied()
        .find(|c| c.is_numeric())
        .or_else(|| if words.len() > 1 { words.last().and_then(|w| w.first().copied()) } else { first_word.last().copied() })
        .unwrap_or(first);
    format!("{first}{second}").to_uppercase().chars().take(2).collect()
}

/// `PROJECT_ICON_COLORS` order: the Tailwind hue a project's name hashes to.
pub const PROJECT_COLORS: [&str; 18] =
    ["gray", "red", "orange", "amber", "yellow", "lime", "green", "emerald", "teal", "cyan", "sky", "blue", "indigo", "violet", "purple", "fuchsia", "pink", "rose"];

pub fn project_color(name: &str) -> usize {
    let seed = name.trim().to_lowercase();
    let seed = if seed.is_empty() { "project".to_string() } else { seed };
    seed.chars().fold(0usize, |i, c| (i * 31 + c as usize) % PROJECT_COLORS.len())
}

/// Search: case-insensitive title or branch matches, title-prefix matches first, then recency.
/// Title and branch hits, then the threads listed only because the server found the query in
/// their messages (`content`, in its order), as `searchSidebarThreads`.
pub fn search(threads: &[Thread], query: &str, content: &[String]) -> Vec<usize> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Vec::new();
    }
    let mut hits: Vec<(u8, i64, usize)> = threads
        .iter()
        .enumerate()
        .filter(|(_, t)| !t.archived && !t.hidden)
        .filter_map(|(i, t)| {
            let title = t.title.to_lowercase();
            let rank = if title.starts_with(&q) {
                0
            } else if title.split(|c: char| !c.is_alphanumeric()).any(|w| w.starts_with(&q)) {
                1
            } else if title.contains(&q) || t.branch.as_deref().is_some_and(|b| b.to_lowercase().contains(&q)) {
                2
            } else {
                return None;
            };
            Some((rank, t.card_time(), i))
        })
        .collect();
    hits.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut out: Vec<usize> = hits.into_iter().map(|h| h.2).collect();
    for id in content {
        if let Some(i) = threads.iter().position(|t| t.id == *id && !t.archived && !t.hidden)
            && !out.contains(&i)
        {
            out.push(i);
        }
    }
    out
}

/// `VISIT_DISPATCH_THROTTLE_MS`: each visit is a full command dispatch and shell broadcast.
pub const VISIT_THROTTLE_MS: i64 = 10_000;

/// The web's `createThreadVisitTracker`: showing a thread records one open, then advances its
/// visit watermark as the shell updates, so finished rows stop reading "Done" once seen.
#[derive(Default)]
pub struct Visits {
    thread: Option<String>,
    open_pending: bool,
    /// A palette pick that already sent the open (with its query) at this `updatedAt`.
    claimed: Option<(String, i64)>,
    last_sent: Option<i64>,
    last_sent_at: i64,
}

impl Visits {
    pub fn claim_open(&mut self, id: &str, updated_at: i64) {
        self.claimed = Some((id.to_string(), updated_at));
    }

    /// The `thread.visit` to send for the shown thread's shell now: `(visitedAt, opened)`.
    pub fn tick(&mut self, shown: Option<&str>, shell: Option<&Value>, now: i64) -> Option<(String, bool)> {
        if self.thread.as_deref() != shown {
            let claimed = self.claimed.take().filter(|(id, _)| Some(id.as_str()) == shown).map(|(_, at)| at);
            *self = Visits {
                thread: shown.map(str::to_string),
                open_pending: claimed.is_none(),
                last_sent: claimed,
                last_sent_at: if claimed.is_some() { now } else { 0 },
                claimed: None,
            };
        }
        let shell = shell?;
        let visited = shell["updatedAt"].as_str()?;
        let updated = iso_ms(visited)?;
        // Servers without visit tracking leave the field out.
        let last_visited = opt_ts(shell.get("lastVisitedAt")?);
        if std::mem::take(&mut self.open_pending) {
            return Some(self.sent(visited, updated, now, true));
        }
        if last_visited.is_some_and(|v| v >= updated) {
            // Seen already, so a later mark-unread (which leaves updatedAt alone) stays sticky.
            if self.last_sent.is_none_or(|s| s < updated) {
                self.last_sent = Some(updated);
            }
            return None;
        }
        if self.last_sent.is_some_and(|s| s >= updated) {
            return None;
        }
        let completed = opt_ts(&shell["latestRunCompletedAt"]);
        let unseen = completed.is_some_and(|c| last_visited.is_none_or(|v| c > v));
        (unseen || now - self.last_sent_at >= VISIT_THROTTLE_MS).then(|| self.sent(visited, updated, now, false))
    }

    fn sent(&mut self, visited: &str, updated: i64, now: i64, opened: bool) -> (String, bool) {
        self.last_sent = Some(updated);
        self.last_sent_at = now;
        (visited.to_string(), opened)
    }
}

/// How long the footer's undo notice stays (`showThreadUndoNotice`).
pub const UNDO_MS: i64 = 5_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UndoAction {
    Settled,
    Snoozed,
    Unpinned,
    Archived,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UndoStep {
    Unsettle,
    Pin(Option<String>),
    Snooze(i64),
    Unsnooze,
    Unarchive,
}

/// `planThreadUndo`: the commands that put a thread back where the action found it.
pub fn undo_plan(action: UndoAction, t: &Thread) -> Vec<UndoStep> {
    let pin = UndoStep::Pin(t.pin_key.clone());
    // Pinning clears snooze on the server, so the snooze comes back after the pin.
    let snooze = t.snoozed_until.map(UndoStep::Snooze);
    match action {
        UndoAction::Snoozed => vec![UndoStep::Unsnooze],
        UndoAction::Archived => vec![UndoStep::Unarchive],
        UndoAction::Unpinned => std::iter::once(pin).chain(snooze).collect(),
        UndoAction::Settled => std::iter::once(UndoStep::Unsettle).chain(t.pinned.then_some(pin)).chain(snooze).collect(),
    }
}

/// The live undoable thread actions behind the footer notice. One timer covers them all; the
/// notice and Undo act on the newest run of the same action.
#[derive(Default)]
pub struct Undos {
    live: Vec<(UndoAction, String, Vec<UndoStep>)>,
    expires: i64,
    /// A new notice owns mod+z, even over a text field, until another key is pressed.
    claimed: bool,
}

impl Undos {
    pub fn push(&mut self, action: UndoAction, id: &str, plan: Vec<UndoStep>, now: i64) {
        // Re-pinning or re-snoozing would un-settle a settled thread, and an archived one
        // leaves the list, so these supersede the thread's older entries.
        // The notice's timer clears everything, as the web's expiry does; a new action never
        // revives an expired one.
        if now >= self.expires {
            self.live.clear();
        }
        let supersedes = |a: UndoAction| a == action || matches!(action, UndoAction::Archived) || (action == UndoAction::Settled && matches!(a, UndoAction::Unpinned | UndoAction::Snoozed));
        self.live.retain(|(a, other, _)| other != id || !supersedes(*a));
        self.live.push((action, id.to_string(), plan));
        self.expires = now + UNDO_MS;
        self.claimed = true;
    }

    pub fn release(&mut self) {
        self.claimed = false;
    }

    pub fn claims_key(&self, now: i64) -> bool {
        self.claimed && self.group_len(now) > 0
    }

    /// Whether `thread.undo` runs: always outside text fields, and inside one only while the
    /// visible notice still claims the key (`editableFocus` in `AppSidebarLayout`).
    pub fn takes_key(&self, now: i64, editable: bool, notice_shown: bool) -> bool {
        self.group_len(now) > 0 && (!editable || (notice_shown && self.claimed))
    }

    fn group_len(&self, now: i64) -> usize {
        if now >= self.expires {
            return 0;
        }
        let Some((latest, ..)) = self.live.last() else { return 0 };
        self.live.iter().rev().take_while(|(a, ..)| a == latest).count()
    }

    pub fn notice(&self, now: i64) -> Option<(UndoAction, usize)> {
        let n = self.group_len(now);
        (n > 0).then(|| (self.live[self.live.len() - 1].0, n))
    }

    /// Takes the newest group for Undo; an expired notice undoes nothing.
    pub fn take(&mut self, now: i64) -> Vec<(String, Vec<UndoStep>)> {
        let n = self.group_len(now);
        if now >= self.expires {
            self.live.clear();
        }
        self.live.split_off(self.live.len() - n).into_iter().map(|(_, id, plan)| (id, plan)).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn undo_groups_the_newest_action_and_restores_pin_then_snooze() {
        let pinned = Thread { pinned: true, pin_key: Some("a0".into()), snoozed_until: Some(9), ..Default::default() };
        assert_eq!(undo_plan(UndoAction::Settled, &pinned), [UndoStep::Unsettle, UndoStep::Pin(Some("a0".into())), UndoStep::Snooze(9)]);
        assert_eq!(undo_plan(UndoAction::Settled, &Thread::default()), [UndoStep::Unsettle]);
        assert_eq!(undo_plan(UndoAction::Unpinned, &Thread::default()), [UndoStep::Pin(None)]);

        let mut u = Undos::default();
        u.push(UndoAction::Unpinned, "a", vec![UndoStep::Pin(None)], 0);
        u.push(UndoAction::Archived, "b", vec![UndoStep::Unarchive], 0);
        u.push(UndoAction::Archived, "c", vec![UndoStep::Unarchive], 1_000);
        assert_eq!(u.notice(1_000), Some((UndoAction::Archived, 2)));
        assert!(u.claims_key(1_000));
        assert!(u.takes_key(1_000, true, true));
        assert!(!u.takes_key(1_000, true, false));
        u.release();
        assert!(!u.claims_key(1_000));
        // After another key, text fields get mod+z back, but elsewhere it still undoes.
        assert!(!u.takes_key(1_000, true, true));
        assert!(u.takes_key(1_000, false, false));
        assert!(!u.takes_key(UNDO_MS + 1_000, false, true));
        let ids: Vec<_> = u.take(1_000).into_iter().map(|(id, _)| id).collect();
        assert_eq!(ids, ["b", "c"]);
        assert_eq!(u.notice(1_000), Some((UndoAction::Unpinned, 1)));
        // Settling supersedes the thread's own unpin undo.
        u.push(UndoAction::Settled, "a", vec![UndoStep::Unsettle], 2_000);
        assert_eq!(u.notice(2_000), Some((UndoAction::Settled, 1)));
        assert_eq!(u.take(2_000).len(), 1);
        assert_eq!(u.notice(2_000), None);
        // The notice expires five seconds after the newest action, taking everything with it.
        u.push(UndoAction::Snoozed, "d", vec![UndoStep::Unsnooze], 3_000);
        assert_eq!(u.notice(3_000 + UNDO_MS), None);
        assert!(u.take(3_000 + UNDO_MS).is_empty());
        u.push(UndoAction::Settled, "e", vec![UndoStep::Unsettle], 20_000);
        u.push(UndoAction::Settled, "f", vec![UndoStep::Unsettle], 30_000);
        assert_eq!(u.notice(30_000), Some((UndoAction::Settled, 1)));
    }

    fn ms(iso: &str) -> i64 {
        iso_ms(iso).unwrap()
    }

    fn runtime(status: &str) -> Option<Runtime> {
        Some(Runtime { status: status.into(), ..Default::default() })
    }

    #[test]
    fn status_prioritizes_input_over_running_below_approval() {
        let t = Thread { pending_input: true, runtime: runtime("running"), blocked_on_wait: true, ..Default::default() };
        assert_eq!(status(&t), Status::Input);
        assert_eq!(status(&Thread { pending_approval: true, ..t }), Status::Approval);
    }

    #[test]
    fn status_shows_guest_request_below_agent_requests() {
        assert_eq!(status(&Thread { guests: 1, runtime: runtime("running"), ..Default::default() }), Status::Request);
        assert_eq!(status(&Thread { guests: 1, pending_input: true, ..Default::default() }), Status::Input);
    }

    #[test]
    fn status_keeps_usage_limit_stops_limited() {
        let limited = Runtime { status: "failed".into(), last_error_class: Some("usage_limit".into()), ..Default::default() };
        let t = |s: &str| Thread { runtime: Some(Runtime { status: s.into(), ..limited.clone() }), ..Default::default() };
        assert_eq!(status(&t("failed")), Status::Limited);
        assert_eq!(status(&t("running")), Status::Working);
        assert_eq!(status(&t("completed")), Status::Ready);
        assert!(!recedes(Status::Limited, false, false, false, false));
    }

    #[test]
    fn status_reports_failed_only_while_latest_run_failed() {
        let t = |s: &str| Thread { runtime: runtime(s), ..Default::default() };
        assert_eq!(status(&t("failed")), Status::Failed);
        assert_eq!(status(&t("completed")), Status::Ready);
        assert_eq!(status(&t("idle")), Status::Waiting);
        assert_eq!(status(&Thread { runtime: runtime("running"), blocked_on_wait: true, ..Default::default() }), Status::Waiting);
    }

    #[test]
    fn unread_needs_a_visit_marker() {
        let run = Some(Run { id: "r".into(), status: "completed".into(), completed_at: Some(ms("2026-03-09T10:05:00Z")), ..Default::default() });
        assert!(!Thread { run: run.clone(), last_visited_at: None, ..Default::default() }.unread());
        assert!(Thread { run: run.clone(), last_visited_at: Some(ms("2026-03-09T10:00:00Z")), ..Default::default() }.unread());
        assert!(!Thread { run: run.clone(), last_visited_at: Some(ms("2026-03-09T10:06:00Z")), ..Default::default() }.unread());
        assert!(Thread { run, last_visited_at: Some(i64::MIN), ..Default::default() }.unread());
    }

    #[test]
    fn visits_open_once_then_advance_the_watermark_throttled_and_keep_mark_unread() {
        let shell = |updated: &str, visited: &str, done: &str| json!({ "updatedAt": updated, "lastVisitedAt": visited, "latestRunCompletedAt": done });
        let (a, b, c) = ("2026-03-09T10:00:00.000Z", "2026-03-09T10:01:00.000Z", "2026-03-09T10:02:00.000Z");
        let mut v = Visits::default();
        assert_eq!(v.tick(None, None, 0), None);
        // Showing a seen thread still records the open.
        assert_eq!(v.tick(Some("t"), Some(&shell(a, a, a)), 1_000), Some((a.into(), true)));
        assert_eq!(v.tick(Some("t"), Some(&shell(a, a, a)), 2_000), None);
        // A finished run the user is looking at is seen at once.
        assert_eq!(v.tick(Some("t"), Some(&shell(b, a, b)), 3_000), Some((b.into(), false)));
        // Not resent before the echo lands.
        assert_eq!(v.tick(Some("t"), Some(&shell(b, a, b)), 4_000), None);
        // Mid-run updates wait out the throttle.
        assert_eq!(v.tick(Some("t"), Some(&shell(c, b, b)), 5_000), None);
        assert_eq!(v.tick(Some("t"), Some(&shell(c, b, b)), 13_000), Some((c.into(), false)));
        // Mark unread rewinds lastVisitedAt without touching updatedAt: it stays unread.
        assert_eq!(v.tick(Some("t"), Some(&shell(c, a, b)), 60_000), None);
        // A palette pick already sent the open.
        v.claim_open("u", ms(a));
        assert_eq!(v.tick(Some("u"), Some(&shell(a, "2026-03-09T09:00:00.000Z", a)), 61_000), None);
        // No visit support on the server: nothing is sent.
        assert_eq!(Visits::default().tick(Some("t"), Some(&json!({ "updatedAt": a })), 0), None);
    }

    #[test]
    fn working_shelf_folds_running_and_background_waits_only() {
        let run = Some(Run { id: "r".into(), status: "completed".into(), ..Default::default() });
        let idle = Thread { run, ..Default::default() };
        let running = Thread { runtime: runtime("running"), ..idle.clone() };
        let waiting = Thread { runtime: runtime("idle"), ..idle.clone() };
        assert!(running.working());
        assert!(waiting.working());
        assert!(!idle.working());
        assert!(!Thread { pending_approval: true, ..running.clone() }.working());
        assert!(!Thread { pending_input: true, ..running }.working());
        assert!(!Thread { runtime: runtime("failed"), ..waiting.clone() }.working());
        assert!(!Thread { plan_mode: true, actionable_plan: true, ..waiting }.working());
    }

    #[test]
    fn shell_parks_background_work_at_idle() {
        let t = Thread::from_shell(&json!({
            "id": "t", "status": "completed", "latestRunId": "r", "activeProviderThreadId": null,
            "updatedAt": "2026-03-09T10:00:00.000Z",
            "pendingBackgroundTasks": [{ "taskId": "bg", "kind": "monitor", "awaited": true }],
        }));
        assert_eq!(t.runtime.as_ref().unwrap().status, "idle");
        assert_eq!(status(&t), Status::Waiting);
        assert_eq!(t.run.unwrap().completed_at, Some(ms("2026-03-09T10:00:00Z")));
        let fresh = Thread::from_shell(&json!({ "id": "n", "status": "idle", "latestRunId": null, "activeProviderThreadId": null }));
        assert!(fresh.runtime.is_none() && fresh.run.is_none());
    }

    #[test]
    fn working_start_ignores_activity_once_newest_run_completed() {
        let run = Run { id: "r".into(), status: "completed".into(), started_at: Some(1), completed_at: Some(5), ..Default::default() };
        let rt = Runtime { status: "running".into(), active_run_id: Some("r".into()), ..Default::default() };
        assert_eq!(Thread { run: Some(run.clone()), runtime: Some(rt.clone()), ..Default::default() }.working_started_at(), None);
        let open = Run { completed_at: None, ..run };
        assert_eq!(Thread { run: Some(open.clone()), runtime: Some(rt.clone()), ..Default::default() }.working_started_at(), Some(1));
        let activity = Runtime { activity_started_at: Some(Some(9)), ..rt };
        assert_eq!(Thread { run: Some(open), runtime: Some(activity), ..Default::default() }.working_started_at(), Some(9));
    }

    #[test]
    fn inbox_puts_the_thread_that_finished_last_on_top() {
        let run = |c: &str, done: &str| Some(Run { id: "r".into(), status: "completed".into(), requested_at: Some(ms(c)), completed_at: Some(ms(done)), ..Default::default() });
        let threads = vec![
            Thread { id: "new".into(), created_at: ms("2026-03-09T11:00:00Z"), ..Default::default() },
            Thread { id: "old-finished-now".into(), created_at: ms("2026-03-01T09:00:00Z"), run: run("2026-03-01T09:00:00Z", "2026-03-09T12:00:00Z"), ..Default::default() },
            Thread { id: "reopened".into(), created_at: ms("2026-03-02T09:00:00Z"), unsettled_at: Some(ms("2026-03-09T11:30:00Z")), ..Default::default() },
        ];
        let s = partition(&threads, 0, true);
        let ids: Vec<&str> = s.active.iter().map(|i| threads[*i].id.as_str()).collect();
        assert_eq!(ids, ["old-finished-now", "reopened", "new"]);
    }

    #[test]
    fn sections_route_snoozed_settled_pinned() {
        let now = ms("2026-03-09T12:00:00Z");
        let threads = vec![
            Thread { id: "s".into(), snoozed_until: Some(now + 1), settled: true, ..Default::default() },
            Thread { id: "woke".into(), snoozed_until: Some(now - 1), settled: true, ..Default::default() },
            Thread { id: "p".into(), pinned: true, ..Default::default() },
            Thread { id: "raised".into(), snoozed_until: Some(now + 1), pending_input: true, ..Default::default() },
            Thread { id: "gone".into(), archived: true, ..Default::default() },
            Thread { id: "sub".into(), hidden: true, ..Default::default() },
        ];
        let s = partition(&threads, now, true);
        assert_eq!((s.snoozed, s.settled, s.pinned, s.active), (vec![0], vec![1], vec![2], vec![3]));
    }

    #[test]
    fn pinned_sorts_keyed_by_key_then_keyless_newest_first() {
        let t = |id: &str, c: &str, k: Option<&str>| Thread { id: id.into(), created_at: ms(c), pin_key: k.map(Into::into), ..Default::default() };
        let threads = vec![
            t("keyless-old", "2026-03-09T08:00:00Z", None),
            t("second", "2026-03-09T09:00:00Z", Some("t")),
            t("keyless-new", "2026-03-09T12:00:00Z", None),
            t("first", "2026-03-09T07:00:00Z", Some("g")),
        ];
        let mut rows = vec![0, 1, 2, 3];
        sort_pinned(&threads, &mut rows);
        assert_eq!(rows, [3, 1, 2, 0]);
        let tied = vec![t("b", "2026-03-09T10:00:00Z", Some("m")), t("a", "2026-03-09T11:00:00Z", Some("m"))];
        let mut rows = vec![0, 1];
        sort_pinned(&tied, &mut rows);
        assert_eq!(rows, [1, 0]);
    }

    #[test]
    fn active_sorts_keyless_first() {
        let t = |id: &str, c: i64, k: Option<&str>| Thread { id: id.into(), created_at: c, active_key: k.map(Into::into), ..Default::default() };
        let threads = vec![t("k2", 1, Some("t")), t("old", 1, None), t("k1", 9, Some("g")), t("new", 5, None)];
        let mut rows = vec![0, 1, 2, 3];
        sort_active(&threads, &mut rows);
        assert_eq!(rows, [3, 1, 2, 0]);
    }

    #[test]
    fn key_between_stays_ordered_under_repeated_top_insertion() {
        let mut head: Option<String> = None;
        let mut seen = HashSet::new();
        for _ in 0..100 {
            let key = key_between(None, head.as_deref()).unwrap();
            if let Some(h) = &head {
                assert!(&key < h);
            }
            seen.insert(key.clone());
            head = Some(key);
        }
        assert_eq!(seen.len(), 100);
    }

    #[test]
    fn key_between_stays_ordered_under_repeated_middle_insertion() {
        let mut low = key_between(None, None).unwrap();
        let mut high = key_between(Some(&low), None).unwrap();
        for i in 0..100 {
            let key = key_between(Some(&low), Some(&high)).unwrap();
            assert!(low < key && key < high);
            if i % 2 == 0 {
                low = key;
            } else {
                high = key;
            }
        }
    }

    #[test]
    fn key_between_rejects_corrupt_or_out_of_order_bounds() {
        assert_eq!(key_between(Some("z"), Some("a")), None);
        assert_eq!(key_between(Some("A!"), None), None);
        assert_eq!(key_between(None, Some("ma")), None);
        assert_eq!(key_between(Some("m"), Some("m")), None);
        assert_eq!(key_between(None, None).as_deref(), Some("n"));
    }

    #[test]
    fn spread_keys_are_ordered_and_valid() {
        let keys = spread_keys(30);
        assert_eq!(keys.len(), 30);
        assert!(keys.windows(2).all(|w| w[0] < w[1]));
        assert!(keys.iter().all(|k| valid_key(k) && k.len() == 2));
        assert_eq!(spread_keys(1), ["nb"]);
    }

    fn keys(pairs: &[(&str, Option<&str>)]) -> Keys {
        pairs.iter().map(|(id, k)| (id.to_string(), k.map(Into::into))).collect()
    }

    fn ids(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn reorder_writes_one_key_between_keyed_neighbours() {
        let k = keys(&[("a", Some("f")), ("b", Some("m")), ("c", Some("t"))]);
        let plan = plan_reorder(&ids(&["a", "c", "b"]), &k, "c");
        assert_eq!(plan.len(), 1);
        assert!("f" < plan[0].1.as_str() && plan[0].1.as_str() < "m");
    }

    #[test]
    fn reorder_spreads_when_a_neighbour_is_keyless_and_skips_hidden_keys() {
        let k = keys(&[("a", None), ("b", None), ("hidden", Some("n"))]);
        let plan = plan_reorder(&ids(&["b", "a"]), &k, "b");
        let written: Vec<&str> = plan.iter().map(|(id, _)| id.as_str()).collect();
        assert_eq!(written, ["b", "a"]);
        assert!(plan[0].1 < plan[1].1);
        assert!(plan.iter().all(|(_, key)| key != "n"));
    }

    fn items() -> Vec<Item> {
        let row = |k: &str, s| Item::Thread { key: k.into(), section: s };
        vec![
            Item::Marker(Marker::PinnedHeader),
            row("p1", Section::Pinned),
            Item::Marker(Marker::PinnedDivider),
            row("a1", Section::Active),
            row("a2", Section::Active),
            Item::Marker(Marker::WorkingHeader),
            row("w1", Section::Working),
            Item::Marker(Marker::SettledHeader),
            row("s1", Section::Settled),
        ]
    }

    #[test]
    fn never_drops_into_working_and_keeps_it_out_of_inbox_order() {
        let items = items();
        assert_eq!(drop_target(&items, "a1", "w1"), None);
        assert_eq!(drop_target(&items, "p1", "a2"), Some(DropTarget { section: Section::Active, pinned_order: vec![], active_order: ids(&["a1", "a2", "p1"]) }));
        assert_eq!(drop_verb(Section::Active, Some(Section::Working)), None);
    }

    #[test]
    fn drop_target_resolves_pin_and_settle_slots() {
        let items = items();
        let t = drop_target(&items, "a2", "p1").unwrap();
        assert_eq!((t.section, t.pinned_order, t.active_order), (Section::Pinned, ids(&["a2", "p1"]), ids(&["a1"])));
        assert_eq!(drop_target(&items, "a1", "s1").unwrap().section, Section::Settled);
        assert_eq!(drop_target(&items, "a1", "sidebar-marker-pinned-divider").unwrap().section, Section::Pinned);
    }

    #[test]
    fn working_threads_never_move_on_drop() {
        let items = items();
        let (pk, ak) = (keys(&[("p1", Some("m"))]), keys(&[("a1", Some("f")), ("a2", Some("t"))]));
        for over in ["p1", "a2", "s1"] {
            let target = drop_target(&items, "w1", over).unwrap();
            let plan = plan_drop(&DropInput {
                active: "w1",
                section: Section::Working,
                target: &target,
                pinned_order: &ids(&["p1"]),
                pinned_keys: &pk,
                pinned_writable: None,
                active_order: &ids(&["a1", "a2"]),
                active_keys: &ak,
                active_writable: None,
                time_ordered: true,
            });
            assert_eq!(plan, Plan::None);
        }
    }

    #[test]
    fn arranges_writable_rows_when_another_cannot_store_an_order() {
        let k = keys(&[("offline", None), ("a1", None), ("a2", None)]);
        let writable: HashSet<String> = ids(&["a1", "a2"]).into_iter().collect();
        let target = DropTarget { section: Section::Active, pinned_order: vec![], active_order: ids(&["offline", "a2", "a1"]) };
        let plan = plan_drop(&DropInput {
            active: "a2",
            section: Section::Active,
            target: &target,
            pinned_order: &[],
            pinned_keys: &Keys::new(),
            pinned_writable: None,
            active_order: &ids(&["offline", "a1", "a2"]),
            active_keys: &k,
            active_writable: Some(&writable),
            time_ordered: false,
        });
        let Plan::MoveActive { assignments, .. } = plan else { panic!("{plan:?}") };
        assert_eq!(assignments.iter().map(|(id, _)| id.as_str()).collect::<Vec<_>>(), ["a2", "a1"]);
        assert!(assignments[0].1 < assignments[1].1);

        let k = keys(&[("offline", Some("m")), ("a2", Some("t"))]);
        let writable: HashSet<String> = ids(&["a2"]).into_iter().collect();
        let target = DropTarget { section: Section::Active, pinned_order: vec![], active_order: ids(&["a2", "offline"]) };
        let above = plan_drop(&DropInput {
            active: "a2",
            section: Section::Active,
            target: &target,
            pinned_order: &[],
            pinned_keys: &Keys::new(),
            pinned_writable: None,
            active_order: &ids(&["offline", "a2"]),
            active_keys: &k,
            active_writable: Some(&writable),
            time_ordered: false,
        });
        assert!(matches!(&above, Plan::MoveActive { assignments, .. } if assignments[0].1.as_str() < "m"));
    }

    #[test]
    fn time_ordered_inbox_only_changes_lifecycle() {
        let target = DropTarget { section: Section::Active, pinned_order: vec![], active_order: ids(&["a1", "p1"]) };
        let empty = Keys::new();
        let input = |section| DropInput {
            active: "p1",
            section,
            target: &target,
            pinned_order: &[],
            pinned_keys: &empty,
            pinned_writable: None,
            active_order: &[],
            active_keys: &empty,
            active_writable: None,
            time_ordered: true,
        };
        assert_eq!(plan_drop(&input(Section::Pinned)), Plan::MoveActive { order: None, assignments: vec![], unpin: true, unsettle: false, unsnooze: false });
        assert_eq!(plan_drop(&input(Section::Active)), Plan::None);
        assert_eq!(plan_drop(&input(Section::Settled)), Plan::MoveActive { order: None, assignments: vec![], unpin: false, unsettle: true, unsnooze: false });
    }

    #[test]
    fn pin_drop_assigns_the_moved_row_a_key() {
        let target = DropTarget { section: Section::Pinned, pinned_order: ids(&["p1", "a1"]), active_order: vec![] };
        let pk = keys(&[("p1", Some("m"))]);
        let plan = plan_drop(&DropInput {
            active: "a1",
            section: Section::Active,
            target: &target,
            pinned_order: &ids(&["p1"]),
            pinned_keys: &pk,
            pinned_writable: None,
            active_order: &[],
            active_keys: &Keys::new(),
            active_writable: None,
            time_ordered: true,
        });
        assert!(matches!(&plan, Plan::Pin { key: Some(k), extra, .. } if k.as_str() > "m" && extra.is_empty()), "{plan:?}");
    }

    #[test]
    fn drop_verbs() {
        use Section::*;
        assert_eq!(drop_verb(Active, Some(Pinned)), Some(Verb::Pin));
        assert_eq!(drop_verb(Pinned, Some(Active)), Some(Verb::Unpin));
        assert_eq!(drop_verb(Settled, Some(Active)), Some(Verb::Unsettle));
        assert_eq!(drop_verb(Snoozed, Some(Active)), Some(Verb::Wake));
        assert_eq!(drop_verb(Active, Some(Settled)), Some(Verb::Settle));
        assert_eq!(drop_verb(Active, Some(Active)), None);
        assert_eq!(drop_verb(Active, None), None);
    }

    #[test]
    fn labels() {
        assert_eq!(relative_label(0, 59_999), "now");
        assert_eq!(relative_label(0, 33 * 60_000 + 5_000), "33m");
        assert_eq!(relative_label(0, 5 * 3_600_000), "5h");
        assert_eq!(relative_label(0, 50 * 3_600_000), "2d");
        assert_eq!(duration_label(42_000), "42s");
        assert_eq!(duration_label(125_000), "2m");
        assert_eq!(duration_label(3_725_000), "1h 2m");
    }

    #[test]
    fn monograms_and_colors() {
        let c = |n: &str| PROJECT_COLORS[project_color(n)];
        assert_eq!((monogram("s5code").as_str(), c("s5code")), ("S5", "cyan"));
        assert_eq!((monogram("rune-site").as_str(), c("rune-site")), ("RS", "green"));
        assert_eq!((monogram("billing-api").as_str(), c("billing-api")), ("BA", "sky"));
        assert_eq!((monogram("server").as_str(), c("server")), ("SR", "amber"));
        assert_eq!(monogram("  --  "), "PR");
        assert_eq!(monogram("a"), "AA");
    }

    #[test]
    fn search_ranks_prefix_then_word_then_substring() {
        let t = |id: &str, title: &str, at: i64| Thread { id: id.into(), title: title.into(), updated_at: at, ..Default::default() };
        let threads = vec![t("sub", "Unsidebar", 9), t("word", "Fix sidebar", 1), t("prefix", "Sidebar v2", 0), t("miss", "Docs", 9)];
        assert_eq!(search(&threads, "side", &[]), [2, 1, 0]);
        assert!(search(&threads, "  ", &[]).is_empty());
        // Message matches follow, in server order, without repeating a title hit.
        let content = ["miss".to_string(), "word".into(), "gone".into()];
        assert_eq!(search(&threads, "side", &content), [2, 1, 0, 3]);
    }
}
