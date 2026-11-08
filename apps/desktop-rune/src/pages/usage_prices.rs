//! Custom model prices (UsagePriceOverrides): this environment's per-model rates and "Map to"
//! aliases, edited as one table and saved as a single `server.updateSettings` patch. Rune talks to
//! one environment, so "Apply to" picks between it and nothing.

use std::collections::{HashMap, HashSet};

use rune::{El, TextEdit, Tx, col, el, row, spacer, text};
use serde_json::{Map, Value, json};

use crate::App;
use crate::icons as i;
use crate::kit::{self, Menu, MenuItem, Placement};
use crate::theme::{self, AlphaMul, T};

/// USAGE_PRICE_FIELDS: settings key, column label, whether a blank cell is allowed.
pub const FIELDS: [(&str, &str, bool); 4] = [
    ("inputCostPerMillionTokens", "Input", false),
    ("outputCostPerMillionTokens", "Output", false),
    ("cacheReadCostPerMillionTokens", "Cache read", true),
    ("cacheWriteCostPerMillionTokens", "Cache write", true),
];

/// The environment the table edits (UsagePriceTarget). `None` maps are unreadable here.
pub struct Target {
    pub label: String,
    pub prices: Option<Map<String, Value>>,
    pub aliases: Option<Map<String, Value>>,
    pub unavailable: Option<String>,
}

#[derive(Clone, Default, Debug, PartialEq)]
pub struct Draft {
    pub id: String,
    pub model: String,
    pub new: bool,
    /// Edited cells only, by `FIELDS` index.
    pub values: [Option<String>; 4],
    /// Edited "Map to"; `None` keeps the saved mapping, blank removes it.
    pub alias: Option<String>,
    pub removed: bool,
}

/// `None` removes the model's custom price or mapping.
#[derive(Clone, Debug, PartialEq)]
pub enum Change {
    Price(String, Option<Value>),
    Alias(String, Option<String>),
}

fn number(v: &Value) -> String {
    match (v.as_i64(), v.as_f64()) {
        (Some(n), _) => n.to_string(),
        (_, Some(f)) => f.to_string(),
        _ => String::new(),
    }
}

fn model_price<'a>(t: &'a Target, model: &str) -> Option<&'a Value> {
    t.prices.as_ref()?.get(model)
}

fn model_alias<'a>(t: &'a Target, model: &str) -> Option<&'a str> {
    t.aliases.as_ref()?.get(model)?.as_str()
}

/// usagePriceForm: a saved price's cells, blank where unset.
pub fn form(price: Option<&Value>) -> [String; 4] {
    FIELDS.map(|(key, _, _)| price.map(|p| number(&p[key])).unwrap_or_default())
}

fn digits(b: &[u8], i: &mut usize) -> usize {
    let start = *i;
    while *i < b.len() && b[*i].is_ascii_digit() {
        *i += 1;
    }
    *i - start
}

/// The partial numbers a price cell accepts while typing, decimal comma and exponent included.
fn typing_number(s: &str) -> bool {
    let b = s.trim().as_bytes();
    let mut i = 0;
    let int = digits(b, &mut i);
    let sep = matches!(b.get(i), Some(b'.' | b','));
    if sep {
        i += 1;
    }
    let frac = digits(b, &mut i);
    if i == b.len() {
        return true;
    }
    if !(int > 0 || (sep && frac > 0)) || !matches!(b[i], b'e' | b'E') {
        return false;
    }
    i += 1;
    if matches!(b.get(i), Some(b'+' | b'-')) {
        i += 1;
    }
    digits(b, &mut i);
    i == b.len()
}

/// isUsagePriceEdit: number-shaped text, or any deletion even if it leaves the cell invalid.
pub fn is_price_edit(previous: &str, next: &str) -> bool {
    if typing_number(next) {
        return true;
    }
    if next.len() >= previous.len() {
        return false;
    }
    let (p, n) = (previous.as_bytes(), next.as_bytes());
    let start = p.iter().zip(n).take_while(|(a, b)| a == b).count();
    n[start..] == p[p.len() - (n.len() - start)..]
}

fn parse_price(raw: &str) -> Option<f64> {
    let raw = raw.trim().replacen(',', ".", 1);
    let b = raw.as_bytes();
    let mut i = 0;
    let int = digits(b, &mut i);
    let dot = b.get(i) == Some(&b'.');
    if dot {
        i += 1;
    }
    let frac = digits(b, &mut i);
    if !(int > 0 || (dot && frac > 0)) {
        return None;
    }
    if i < b.len() {
        if !matches!(b[i], b'e' | b'E') {
            return None;
        }
        i += 1;
        if matches!(b.get(i), Some(b'+' | b'-')) {
            i += 1;
        }
        if digits(b, &mut i) == 0 {
            return None;
        }
    }
    let value: f64 = if i == b.len() { raw.parse().ok()? } else { return None };
    (value.is_finite() && value >= 0.0).then_some(value)
}

/// parseUsagePriceForm: blank cache rates fall back to the input rate; zero means free.
fn parse_form(model: &str, cells: &[String; 4]) -> Option<Value> {
    if model.trim().is_empty() {
        return None;
    }
    let mut price = Map::new();
    for ((key, _, optional), raw) in FIELDS.iter().zip(cells) {
        if raw.trim().is_empty() {
            if *optional {
                continue;
            }
            return None;
        }
        price.insert((*key).into(), json!(parse_price(raw)?));
    }
    Some(Value::Object(price))
}

pub fn is_empty(d: &Draft) -> bool {
    d.new && d.model.trim().is_empty() && d.alias.as_deref().unwrap_or("").trim().is_empty() && d.values.iter().all(|v| v.as_deref().unwrap_or("").trim().is_empty())
}

/// usageAliasCell: the saved mapping and what an empty cell says.
pub fn alias_cell(t: &Target, model: &str) -> (String, &'static str) {
    match &t.aliases {
        None => (String::new(), "Unavailable"),
        Some(_) => (model_alias(t, model).unwrap_or("").to_string(), "None"),
    }
}

/// usagePriceCell: the saved rate and what an empty cell says.
pub fn price_cell(t: &Target, model: &str, field: usize) -> (String, &'static str) {
    if t.prices.is_none() {
        return (String::new(), "Unavailable");
    }
    match model_price(t, model) {
        None => (String::new(), "Automatic"),
        Some(price) => (form(Some(price))[field].clone(), if FIELDS[field].2 { "Input rate" } else { "0.00" }),
    }
}

/// usagePriceTableChanges: the patch entries `drafts` make on `t`, and each draft's error.
pub fn changes(t: &Target, drafts: &[Draft]) -> (Vec<Change>, HashMap<String, String>) {
    let mut out = Vec::new();
    let mut errors = HashMap::new();
    for d in drafts {
        if is_empty(d) {
            continue;
        }
        let model = d.model.trim().to_string();
        let current = model_alias(t, &model).map(str::to_string);
        if d.removed {
            if model_price(t, &model).is_some() {
                out.push(Change::Price(model.clone(), None));
            }
            if current.is_some() {
                out.push(Change::Alias(model, None));
            }
            continue;
        }
        let alias = d.alias.clone().or_else(|| current.clone()).unwrap_or_default().trim().to_string();
        if !alias.is_empty() {
            if d.alias.is_none() && d.values.iter().any(Option::is_some) {
                errors.insert(d.id.clone(), format!("{} maps this model. Remove it from \"Apply to\" or clear its mapping first.", t.label));
            } else if model.is_empty() {
                errors.insert(d.id.clone(), "Enter a model ID.".into());
            } else if alias == model {
                errors.insert(d.id.clone(), "Map to a different model.".into());
            } else {
                if current.as_ref() != Some(&alias) {
                    out.push(Change::Alias(model.clone(), Some(alias)));
                }
                if model_price(t, &model).is_some() {
                    out.push(Change::Price(model, None));
                }
            }
            continue;
        }
        if current.is_some() {
            out.push(Change::Alias(model.clone(), None));
            if model_price(t, &model).is_none() && d.values.iter().all(|v| v.as_deref().unwrap_or("").trim().is_empty()) {
                continue;
            }
        }
        let original = form(model_price(t, &model));
        let cells: [String; 4] = std::array::from_fn(|f| d.values[f].clone().unwrap_or_else(|| original[f].clone()));
        match parse_form(&model, &cells) {
            Some(price) => {
                if form(Some(&price)) != original {
                    out.push(Change::Price(model, Some(price)));
                }
            }
            None => {
                let missing = FIELDS.iter().zip(&cells).find(|((_, _, optional), cell)| !optional && cell.trim().is_empty());
                let error = match missing {
                    _ if model.is_empty() => "Enter a model ID.".to_string(),
                    Some(((_, label, _), _)) => format!("{label} is required on {}.", t.label),
                    None => "Use non-negative numbers for prices.".into(),
                };
                errors.insert(d.id.clone(), error);
            }
        }
    }
    // The final batch decides cycles, so clearing a mapping and reversing it in one save is fine.
    let mut aliases: HashMap<String, String> = t.aliases.iter().flatten().filter_map(|(k, v)| Some((k.clone(), v.as_str()?.to_string()))).collect();
    for change in &out {
        if let Change::Alias(model, alias) = change {
            match alias {
                Some(alias) => aliases.insert(model.clone(), alias.clone()),
                None => aliases.remove(model),
            };
        }
    }
    for d in drafts {
        if d.removed || errors.contains_key(&d.id) {
            continue;
        }
        let mut model = d.model.trim().to_string();
        let mut seen = HashSet::new();
        while let Some(next) = aliases.get(&model) {
            if !seen.insert(model.clone()) {
                errors.insert(d.id.clone(), format!("Model mappings cannot form a cycle on {}.", t.label));
                break;
            }
            model = next.clone();
        }
    }
    (out, errors)
}

/// The settings patch for `changes`, touching only the maps it names.
pub fn patch(changes: &[Change]) -> Value {
    let (mut prices, mut aliases) = (Map::new(), Map::new());
    for change in changes {
        match change {
            Change::Price(model, price) => prices.insert(model.clone(), price.clone().unwrap_or(Value::Null)),
            Change::Alias(model, alias) => aliases.insert(model.clone(), alias.clone().map_or(Value::Null, Value::from)),
        };
    }
    let mut patch = Map::new();
    if !prices.is_empty() {
        patch.insert("usagePriceOverrides".into(), Value::Object(prices));
    }
    if !aliases.is_empty() {
        patch.insert("usageModelAliases".into(), Value::Object(aliases));
    }
    Value::Object(patch)
}

// ---- state ------------------------------------------------------------------------------

/// A cell's editor, the saved text it was last synced to, and its last accepted text.
#[derive(Default)]
struct Cell {
    edit: TextEdit,
    baseline: String,
    last: String,
}

enum Status {
    Saving,
    Saved,
    Failed(String),
}

struct Attempt {
    drafts: Vec<Draft>,
    status: Status,
}

/// Cell slots: the new row's model ID, "Map to", then `FIELDS`.
const MODEL: usize = 0;
const ALIAS: usize = 1;
const PRICE: usize = 2;

#[derive(Default)]
pub struct Ui {
    pub open: bool,
    /// "Apply to" unchecked this environment.
    deselected: bool,
    cells: HashMap<(String, usize), Cell>,
    new_rows: Vec<String>,
    removed: HashSet<String>,
    next: u64,
    pending: Option<u64>,
    attempt: Option<Attempt>,
    menu: bool,
}

fn ui(s: &mut App) -> &mut Ui {
    &mut s.ui.pages.usage.prices
}

pub fn target(s: &App) -> Target {
    let config = s.state().config.as_ref();
    let label = config.and_then(|c| c["environment"]["label"].as_str()).unwrap_or("This environment").to_string();
    let settings = config.map(|c| &c["settings"]).filter(|v| v.is_object());
    let prices = settings.map(|v| v["usagePriceOverrides"].as_object().cloned().unwrap_or_default());
    let mapped = config.is_some_and(|c| c["environment"]["capabilities"]["usageModelAliases"] == true);
    let aliases = settings.filter(|_| mapped).map(|v| v["usageModelAliases"].as_object().cloned().unwrap_or_default());
    let unavailable = settings.is_none().then(|| "Prices not loaded".to_string());
    Target { label, prices, aliases, unavailable }
}

/// Models with a saved price or mapping, sorted.
fn custom_models(t: &Target) -> Vec<String> {
    let mut models: Vec<String> = t.prices.iter().chain(&t.aliases).flat_map(|m| m.keys().cloned()).collect();
    models.sort();
    models.dedup();
    models
}

fn text_of(u: &Ui, id: &str, slot: usize) -> String {
    u.cells.get(&(id.to_string(), slot)).map(|c| c.edit.text.clone()).unwrap_or_default()
}

/// (row id, saved model or `None` for a new row), saved rows first.
fn rows(u: &Ui, t: &Target) -> Vec<(String, Option<String>)> {
    let fresh: HashSet<String> = u.new_rows.iter().map(|id| text_of(u, id, MODEL).trim().to_string()).collect();
    let saved = custom_models(t).into_iter().filter(|m| u.attempt.is_none() || !fresh.contains(m)).map(|m| (format!("model:{m}"), Some(m)));
    saved.chain(u.new_rows.iter().map(|id| (id.clone(), None))).collect()
}

fn drafts(u: &Ui, t: &Target) -> Vec<Draft> {
    let mut out = Vec::new();
    for (id, saved) in rows(u, t) {
        let cell = |slot: usize| text_of(u, &id, slot);
        let draft = match saved {
            None => Draft {
                model: cell(MODEL),
                new: true,
                values: std::array::from_fn(|f| Some(cell(PRICE + f)).filter(|v| !v.is_empty())),
                alias: Some(cell(ALIAS)).filter(|v| !v.is_empty()),
                ..Draft { id: id.clone(), ..Draft::default() }
            },
            Some(model) => {
                let values = std::array::from_fn(|f| Some(cell(PRICE + f)).filter(|v| *v != price_cell(t, &model, f).0));
                let alias = Some(cell(ALIAS)).filter(|v| *v != alias_cell(t, &model).0);
                let removed = u.removed.contains(&id);
                if !removed && alias.is_none() && values.iter().all(Option::is_none) {
                    continue;
                }
                Draft { id: id.clone(), model, new: false, values, alias, removed }
            }
        };
        out.push(draft);
    }
    out.retain(|d| !is_empty(d));
    out
}

/// Every row's editors exist, and untouched ones follow the saved settings as they change.
fn sync(s: &mut App) {
    let t = target(s);
    let u = ui(s);
    for (id, saved) in rows(u, &t) {
        let mut slots = vec![(ALIAS, saved.as_ref().map(|m| alias_cell(&t, m).0).unwrap_or_default())];
        slots.extend((0..4).map(|f| (PRICE + f, saved.as_ref().map(|m| price_cell(&t, m, f).0).unwrap_or_default())));
        if saved.is_none() {
            slots.push((MODEL, String::new()));
        }
        for (slot, value) in slots {
            let cell = u.cells.entry((id.clone(), slot)).or_insert_with(|| {
                let mut edit = TextEdit::default();
                edit.replace_text(value.clone());
                Cell { edit, baseline: value.clone(), last: value.clone() }
            });
            if saved.is_some() && cell.edit.text == cell.baseline && cell.baseline != value {
                cell.edit.replace_text(value.clone());
                cell.baseline = value.clone();
                cell.last = value;
            }
        }
    }
}

fn add_row(u: &mut Ui, model: &str) -> String {
    let id = format!("new:{}", u.next);
    u.next += 1;
    let mut edit = TextEdit::default();
    edit.replace_text(model.to_string());
    edit.focused = model.is_empty();
    u.cells.insert((id.clone(), MODEL), Cell { edit, baseline: String::new(), last: model.to_string() });
    u.new_rows.push(id.clone());
    id
}

/// Opens the table; `model` (the model dialog's "Set price") starts a row unless it has one.
pub fn open(s: &mut App, model: Option<&str>) {
    let has_row = model.is_some_and(|m| custom_models(&target(s)).iter().any(|c| c == m));
    let u = ui(s);
    *u = Ui { open: true, ..Ui::default() };
    if let Some(model) = model.filter(|_| !has_row) {
        add_row(u, model);
        u.cells.get_mut(&("new:0".to_string(), MODEL)).map(|c| c.edit.focused = false);
    }
    sync(s);
}

fn close(s: &mut App) {
    if ui(s).pending.is_none() {
        *ui(s) = Ui::default();
    }
}

fn discard(s: &mut App) {
    let u = ui(s);
    u.cells.clear();
    u.new_rows.clear();
    u.removed.clear();
    u.attempt = None;
    sync(s);
}

fn failed(u: &Ui) -> bool {
    matches!(u.attempt, Some(Attempt { status: Status::Failed(_), .. }))
}

fn edited(s: &mut App, key: &(String, usize)) {
    let u = ui(s);
    if let Some(cell) = u.cells.get_mut(key) {
        if key.1 >= PRICE && !is_price_edit(&cell.last, &cell.edit.text) {
            let last = cell.last.clone();
            cell.edit.replace_text(last);
            return;
        }
        cell.last = cell.edit.text.clone();
    }
    if u.pending.is_none() {
        u.attempt = None;
    }
}

/// writeUsagePrices for the one destination; a retry resends the failed drafts.
fn save(s: &mut App) {
    let t = target(s);
    let u = ui(s);
    if u.pending.is_some() || u.deselected {
        return;
    }
    let staged = match &u.attempt {
        Some(Attempt { drafts, status: Status::Failed(_) }) => drafts.clone(),
        _ => drafts(u, &t),
    };
    if staged.is_empty() {
        return;
    }
    let (changes, errors) = changes(&t, &staged);
    let unavailable = t.unavailable.clone().or_else(|| errors.values().next().cloned());
    let status = if let Some(reason) = unavailable {
        Status::Failed(reason)
    } else if t.aliases.is_none() && changes.iter().any(|c| matches!(c, Change::Alias(..))) {
        Status::Failed("Update server to map models".into())
    } else if changes.is_empty() {
        Status::Saved
    } else {
        match s.src.net() {
            Some(net) => {
                let id = net.request("server.updateSettings", json!({"patch": patch(&changes)}));
                ui(s).pending = Some(id);
                Status::Saving
            }
            None => Status::Failed("Offline".into()),
        }
    };
    let saved = matches!(status, Status::Saved);
    ui(s).attempt = Some(Attempt { drafts: staged, status });
    if saved {
        settle(s);
    }
}

/// A save landed: drafts clear and the summary refetches under the new prices.
fn settle(s: &mut App) {
    let u = ui(s);
    u.cells.clear();
    u.new_rows.clear();
    u.removed.clear();
    s.ui.pages.usage.requested = None;
    sync(s);
}

pub fn poll(s: &mut App) -> bool {
    if !ui(s).open {
        return false;
    }
    let mut changed = false;
    if let Some(id) = ui(s).pending
        && let Some(reply) = s.src.net().and_then(|n| n.take_reply(id))
    {
        let u = ui(s);
        u.pending = None;
        let ok = reply.is_ok();
        if let Some(attempt) = &mut u.attempt {
            attempt.status = if ok { Status::Saved } else { Status::Failed("Could not save. Try again.".into()) };
        }
        if ok {
            settle(s);
        }
        changed = true;
    }
    let before = ui(s).cells.len();
    sync(s);
    changed || ui(s).cells.len() != before
}

pub fn on_key(s: &mut App, e: &rune::KeyEvent) -> bool {
    if !ui(s).open || !matches!(e.key, rune::Key::Escape) {
        return false;
    }
    if ui(s).menu {
        ui(s).menu = false;
    } else {
        close(s);
    }
    true
}

// ---- view -------------------------------------------------------------------------------

pub fn view(s: &App, t: &T, overlays: &mut Vec<El>) {
    let u = &s.ui.pages.usage.prices;
    if !u.open {
        return;
    }
    let k = kit::theme(t);
    let target = target(s);
    let muted = t.c.muted_foreground;
    let staged = drafts(u, &target);
    let customs = custom_models(&target);
    let mut errors = if target.unavailable.is_none() && target.prices.is_some() { changes(&target, &staged).1 } else { HashMap::new() };
    for d in staged.iter().filter(|d| d.new) {
        let model = d.model.trim();
        if model.is_empty() {
            errors.insert(d.id.clone(), "Enter a model ID.".into());
        } else if u.attempt.is_none() && (customs.iter().any(|c| c == model) || staged.iter().any(|o| o.id != d.id && o.model.trim() == model)) {
            errors.insert(d.id.clone(), "This model already has a row. Edit its prices there.".into());
        }
    }
    let failed = failed(u);
    let pending = u.pending.is_some();
    let locked = pending || failed;
    let has_changes = !staged.is_empty();
    let selected = !u.deselected;
    let destination = if selected { target.label.clone() } else { "0 environments".into() };
    let selection = if selected { "All environments".to_string() } else { destination.clone() };

    let apply = Menu::new("usage-prices-apply")
        .trigger(
            kit::Button::new(selection)
                .variant("outline")
                .size("sm")
                .disabled(pending || has_changes)
                .on_press(|s: &mut App| ui(s).menu = !ui(s).menu)
                .build(t),
        )
        .placement(Placement::BottomStart)
        .min_width(220.0)
        .open(u.menu)
        .on_close(|s: &mut App| ui(s).menu = false)
        .item(MenuItem::new("All environments").checked(selected).on_select(|s: &mut App| {
            let u = ui(s);
            u.deselected = !u.deselected;
            u.attempt = None;
        }))
        .separator()
        .item({
            let item = MenuItem::new(target.label.clone()).checked(selected).on_select(|s: &mut App| {
                let u = ui(s);
                u.deselected = !u.deselected;
                u.attempt = None;
            });
            match &target.unavailable {
                Some(reason) => item.shortcut(reason.clone()),
                None => item,
            }
        });
    let mut body = col()
        .w_full()
        .gap(16.0)
        .child(
            row()
                .w_full()
                .gap(8.0)
                .center_y()
                .child(text("Apply to").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(apply.build(&k, overlays))
                .child(spacer())
                .child(text("USD / million tokens").font(theme::TEXT_XS).color(muted)),
        );
    if !selected {
        let line = if s.state().config.is_none() { "Connect an environment to set model prices." } else { "Select an environment to see and change its model prices." };
        body = body.child(text(line).font(theme::TEXT_SM).color(muted));
    } else {
        body = body.child(table(s, t, u, &target, &staged, &errors, locked)).child(
            text(
                "Blank cache rates use the input price. Enter 0 for free tokens. A mapped model\u{2019}s usage counts as the model it maps to.",
            )
            .font(theme::TEXT_XS)
            .color(muted)
            .wrap(),
        );
    }
    if let Some(attempt) = &u.attempt {
        let (status, ink) = match &attempt.status {
            Status::Failed(error) => (format!("Not saved \u{b7} {error}"), t.c.destructive),
            Status::Saved => ("Saved".into(), muted),
            Status::Saving => ("Saving\u{2026}".into(), muted),
        };
        body = body.child(
            row()
                .w_full()
                .gap(12.0)
                .child(text(target.label.clone()).font(theme::TEXT_XS).color(muted).truncate().shrink())
                .child(spacer())
                .child(text(status).font(theme::TEXT_XS).color(ink)),
        );
    }
    let mut actions = row().gap(8.0).center_y();
    if has_changes {
        actions = actions.child(
            kit::Button::new(if failed { "Discard pending changes" } else { "Discard changes" })
                .variant("ghost")
                .disabled(pending)
                .on_press(discard)
                .build(t),
        );
    }
    let blocked = pending || (!has_changes && !failed) || (!failed && (!errors.is_empty() || !selected));
    actions = actions.child(
        kit::Button::new(if pending {
            "Saving\u{2026}"
        } else if failed {
            "Retry failed saves"
        } else {
            "Save changes"
        })
        .disabled(blocked)
        .on_press(save)
        .build(t),
    );
    let note = if has_changes { format!("Changes apply to {destination}") } else { String::new() };
    body = body.child(row().w_full().gap(8.0).center_y().child(text(note).font(theme::TEXT_XS).color(muted)).child(spacer()).child(actions));
    kit::Dialog::new("usage-prices", "Custom model prices")
        .description("Prices and mappings apply to all past and future usage on the environments you select.")
        .child(body)
        .width(896.0)
        .open(true)
        .on_close(close)
        .build(t, overlays);
}

const MODEL_W: f32 = 180.0;
const ALIAS_W: f32 = 150.0;
const ACTION_W: f32 = 28.0;

fn table(s: &App, t: &T, u: &Ui, target: &Target, staged: &[Draft], errors: &HashMap<String, String>, locked: bool) -> El {
    let muted = t.c.muted_foreground;
    let head = |label: &str| text(label.to_string()).font(theme::XS_MEDIUM).color(muted);
    let mut header = row()
        .w_full()
        .px(12.0)
        .py(8.0)
        .gap(8.0)
        .center_y()
        .child(row().w(MODEL_W).child(head("Model ID")))
        .child(row().w(ALIAS_W).child(head("Map to")));
    for (_, label, _) in FIELDS {
        header = header.child(row().grow(1.0).w(0.0).min_w(0.0).child(head(label)));
    }
    header = header.child(
        row().w(ACTION_W).child(
            kit::Button::new("Add model price")
                .variant("ghost")
                .size("icon-xs")
                .icon(&i::PLUS)
                .disabled(locked)
                .on_press(|s: &mut App| {
                    add_row(ui(s), "");
                    sync(s);
                })
                .build(t),
        ),
    );
    let mut table = col().w_full().radius(radius()).border(1.0, t.c.border).clip_rounded(radius()).child(header).child(el().w_full().h(1.0).fill(t.c.border));
    let rows = rows(u, target);
    if rows.is_empty() {
        let line = if target.prices.is_none() { "Some environment prices are unavailable." } else { "No custom prices or mappings. Add a row to set one." };
        return table.child(row().w_full().py(24.0).center().child(text(line).font(theme::TEXT_SM).color(muted)));
    }
    let _ = s;
    for (n, (id, saved)) in rows.iter().enumerate() {
        if n > 0 {
            table = table.child(el().w_full().h(1.0).fill(t.c.border.alpha_mul(0.6)));
        }
        let draft = staged.iter().find(|d| &d.id == id);
        let removed = draft.is_some_and(|d| d.removed);
        let model = saved.clone().unwrap_or_else(|| text_of(u, id, MODEL));
        let cell = |slot: usize, placeholder: &str, label: String, mono: bool, invalid: bool| -> El {
            let key = (id.clone(), slot);
            let Some(c) = u.cells.get(&key) else { return el().w_full().h(28.0) };
            let (edit_key, change_key) = (key.clone(), key.clone());
            kit::Input::new(&c.edit)
                .size("compact")
                .placeholder(placeholder)
                .font(if mono { "mono" } else { "sans" })
                .invalid(invalid)
                .disabled(locked)
                .on_edit(move |s: &mut App| &mut ui(s).cells.entry(edit_key.clone()).or_default().edit)
                .on_change(move |s: &mut App, _: &mut Tx| edited(s, &change_key))
                .build(t)
                .key(format!("usage-price-{}-{slot}", id))
                .single_line()
                .label(label)
        };
        let error = errors.get(id).filter(|_| !model.trim().is_empty() || draft.is_some_and(|d| d.values.iter().flatten().any(|v| !v.is_empty())));
        let mut model_col = col().w(MODEL_W).gap(4.0);
        model_col = match saved {
            None => model_col.child(cell(MODEL, "Model ID", "New model ID".into(), false, !model.trim().is_empty() && errors.contains_key(id))),
            Some(name) => model_col.child(row().h(28.0).center_y().child(text(name.clone()).font(theme::TEXT_SM).color(if removed { muted } else { t.c.foreground }).wrap())),
        };
        if let Some(error) = error {
            model_col = model_col.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
        }
        let shown = if model.is_empty() { "new model".to_string() } else { model.clone() };
        let mut line = row().w_full().px(12.0).py(8.0).gap(8.0).items_start().child(model_col);
        if removed {
            line = line.child(row().grow(1.0).h(28.0).center_y().child(text("Automatic pricing after saving").font(theme::TEXT_SM).color(muted)));
        } else {
            let (saved_alias, alias_placeholder) = saved.as_ref().map(|m| alias_cell(target, m)).unwrap_or((String::new(), "Optional"));
            let alias = draft.and_then(|d| d.alias.clone()).unwrap_or(saved_alias).trim().to_string();
            line = line.child(row().w(ALIAS_W).child(cell(ALIAS, alias_placeholder, format!("Map {shown} to model"), false, false)));
            if !alias.is_empty() {
                line = line.child(row().grow(1.0).w(0.0).min_w(0.0).h(28.0).center_y().child(text(format!("Counted as {alias}")).font(theme::TEXT_SM).color(muted).truncate()));
            } else {
                for (f, (_, label, optional)) in FIELDS.iter().enumerate() {
                    let placeholder = match saved {
                        None if *optional => "Input rate",
                        None => "0.00",
                        Some(m) => price_cell(target, m, f).1,
                    };
                    line = line.child(row().grow(1.0).w(0.0).min_w(0.0).child(cell(PRICE + f, placeholder, format!("{label} price for {shown}"), true, false)));
                }
            }
        }
        let (glyph, label) = match (saved, removed) {
            (None, _) => (&i::X, "Remove new model".to_string()),
            (Some(m), true) => (&i::ROTATE_CCW, format!("Undo reset for {m}")),
            (Some(m), false) => (&i::ROTATE_CCW, format!("Reset price for {m} to automatic")),
        };
        let row_id = id.clone();
        let is_new = saved.is_none();
        line = line.child(
            row().w(ACTION_W).h(28.0).center_y().child(
                kit::Button::new(label)
                    .variant("ghost")
                    .size("icon-xs")
                    .icon(glyph)
                    .disabled(locked)
                    .on_press(move |s: &mut App| {
                        let u = ui(s);
                        if is_new {
                            u.new_rows.retain(|r| r != &row_id);
                            u.cells.retain(|(r, _), _| r != &row_id);
                        } else if !u.removed.remove(&row_id) {
                            u.removed.insert(row_id.clone());
                        }
                        u.attempt = None;
                    })
                    .build(t),
            ),
        );
        table = table.child(line);
    }
    table
}

fn radius() -> f32 {
    theme::radius::LG
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target(prices: Value, aliases: Option<Value>) -> Target {
        Target { label: "Mac".into(), prices: prices.as_object().cloned(), aliases: aliases.and_then(|a| a.as_object().cloned()), unavailable: None }
    }

    fn draft(id: &str, model: &str, new: bool) -> Draft {
        Draft { id: id.into(), model: model.into(), new, ..Draft::default() }
    }

    #[test]
    fn price_cells_accept_numbers_in_progress_and_any_deletion() {
        for ok in ["", "1", "1.", ".5", "1,5", "2e", "2e-", "2E+3", " 3 "] {
            assert!(is_price_edit("", ok), "{ok}");
        }
        assert!(!is_price_edit("1", "1a"));
        assert!(!is_price_edit("", "e5"));
        assert!(!is_price_edit("1.2", "1.2.3"));
        // Deleting from an invalid value is allowed so the cell can be cleared.
        assert!(is_price_edit("1a2", "12") && is_price_edit("abc", "ac"));
        assert!(!is_price_edit("abc", "xb"));
    }

    #[test]
    fn rates_parse_with_a_decimal_comma_and_blank_cache_rates_are_omitted() {
        let price = parse_form("m", &["1,5".into(), "2e1".into(), "".into(), "0".into()]).unwrap();
        assert_eq!(price, json!({"inputCostPerMillionTokens": 1.5, "outputCostPerMillionTokens": 20.0, "cacheWriteCostPerMillionTokens": 0.0}));
        assert_eq!(form(Some(&price)), ["1.5", "20", "", "0"]);
        assert!(parse_form("m", &["1".into(), "".into(), "".into(), "".into()]).is_none());
        assert!(parse_form("m", &["-1".into(), "2".into(), "".into(), "".into()]).is_none());
        assert!(parse_form(" ", &["1".into(), "2".into(), "".into(), "".into()]).is_none());
    }

    #[test]
    fn edits_become_one_patch_of_prices_and_mappings() {
        let t = target(json!({"kept": {"inputCostPerMillionTokens": 1, "outputCostPerMillionTokens": 2}, "gone": {"inputCostPerMillionTokens": 3, "outputCostPerMillionTokens": 4}}), Some(json!({"old": "kept"})));
        let mut fresh = draft("new:0", "drift-9", true);
        fresh.values = [Some("3".into()), Some("15".into()), None, None];
        let mut edited = draft("model:kept", "kept", false);
        edited.values[0] = Some("1".into());
        let mut mapped = draft("new:1", "orbit", true);
        mapped.alias = Some("kept".into());
        let gone = Draft { removed: true, ..draft("model:gone", "gone", false) };
        let unmapped = Draft { alias: Some(String::new()), ..draft("model:old", "old", false) };
        let (changes, errors) = changes(&t, &[fresh, edited, mapped, gone, unmapped]);
        assert!(errors.is_empty(), "{errors:?}");
        assert_eq!(
            patch(&changes),
            json!({
                "usagePriceOverrides": {"drift-9": {"inputCostPerMillionTokens": 3.0, "outputCostPerMillionTokens": 15.0}, "gone": null},
                "usageModelAliases": {"orbit": "kept", "old": null}
            })
        );
    }

    #[test]
    fn invalid_rows_say_why_on_the_environment() {
        let t = target(json!({}), Some(json!({"a": "b", "mapped": "x"})));
        let missing = Draft { values: [Some("1".into()), None, None, None], ..draft("new:0", "m", true) };
        let negative = Draft { values: [Some("-1".into()), Some("1".into()), None, None], ..draft("new:1", "n", true) };
        let selfish = Draft { alias: Some("s".into()), ..draft("new:2", "s", true) };
        let cycle = Draft { alias: Some("a".into()), ..draft("new:3", "b", true) };
        let priced_while_mapped = Draft { values: [Some("1".into()), None, None, None], ..draft("model:mapped", "mapped", false) };
        let (_, errors) = changes(&t, &[missing, negative, selfish, cycle, priced_while_mapped]);
        assert_eq!(errors["new:0"], "Output is required on Mac.");
        assert_eq!(errors["new:1"], "Use non-negative numbers for prices.");
        assert_eq!(errors["new:2"], "Map to a different model.");
        assert_eq!(errors["new:3"], "Model mappings cannot form a cycle on Mac.");
        assert_eq!(errors["model:mapped"], "Mac maps this model. Remove it from \"Apply to\" or clear its mapping first.");
    }

    #[test]
    fn saved_cells_name_what_a_blank_means() {
        let t = target(json!({"m": {"inputCostPerMillionTokens": 0.25, "outputCostPerMillionTokens": 2}}), None);
        assert_eq!(price_cell(&t, "m", 0), ("0.25".into(), "0.00"));
        assert_eq!(price_cell(&t, "m", 2), (String::new(), "Input rate"));
        assert_eq!(price_cell(&t, "other", 1), (String::new(), "Automatic"));
        assert_eq!(alias_cell(&t, "m"), (String::new(), "Unavailable"));
        let unreadable = Target { prices: None, ..target(json!({}), None) };
        assert_eq!(price_cell(&unreadable, "m", 0).1, "Unavailable");
    }
}
