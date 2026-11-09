//! UsageModelTagDialog: maps an unpriced model name onto a models.dev catalog entry instead of
//! guessing. Tags live in client settings (`usageModelTags`), so one tag covers every environment.

use rune::{El, ScrollSpec, TextEdit, col, row, text};
use serde_json::{Value, json};

use crate::App;
use crate::data::s as field;
use crate::kit;
use crate::theme::{self, AlphaMul, T, radius};

const SETTING: &str = "usageModelTags";

/// UsageModelTagTarget: the breakdown row the dialog is editing.
#[derive(Clone, Debug, PartialEq)]
pub struct Target {
    pub provider: String,
    pub label: String,
    /// The `(apiProvider, model)` transcript identity of a row not tagged yet. A tagged row merges
    /// every name pointed at its catalog entry, so it is addressed by that entry instead.
    pub untagged: Option<(String, String)>,
    pub tagged_as: Option<String>,
}

#[derive(Default)]
pub struct Ui {
    pub target: Option<Target>,
    pub query: TextEdit,
    /// The query the shown models answer, or are on their way for.
    asked: String,
    pending: Option<u64>,
    pub models: Vec<Value>,
}

fn ui(s: &mut App) -> &mut Ui {
    &mut s.ui.pages.usage.tags
}

pub fn open(s: &mut App, target: Target) {
    let u = ui(s);
    u.target = Some(target);
    u.query = TextEdit::default();
    u.query.focused = true;
    u.asked.clear();
    u.models.clear();
}

fn close(s: &mut App) {
    let u = ui(s);
    u.target = None;
    u.query.clear();
}

/// Searches `server.searchUsageModels` for the typed query, one request at a time so a burst of
/// keystrokes collapses into the latest query.
pub fn poll(s: &mut App) -> bool {
    let mut changed = false;
    if let Some(id) = s.ui.pages.usage.tags.pending
        && let Some(reply) = s.src.net().and_then(|n| n.take_reply(id))
    {
        let u = ui(s);
        u.pending = None;
        u.models = reply.ok().and_then(|r| r["models"].as_array().cloned()).unwrap_or_default();
        changed = true;
    }
    let u = &s.ui.pages.usage.tags;
    if u.target.is_none() || u.pending.is_some() || u.query.text == u.asked {
        return changed;
    }
    let query = u.query.text.clone();
    let pending = if query.trim().is_empty() {
        None
    } else {
        let Some(net) = s.src.net() else { return changed };
        Some(net.request("server.searchUsageModels", json!({"query": query})))
    };
    let u = ui(s);
    u.pending = pending;
    u.asked = query;
    u.models.clear();
    true
}

pub fn on_key(s: &mut App, e: &rune::KeyEvent) -> bool {
    if s.ui.pages.usage.tags.target.is_none() || !matches!(e.key, rune::Key::Escape) {
        return false;
    }
    close(s);
    true
}

/// matchesTarget: an untagged target owns one transcript name; a tagged one owns every name the
/// user pointed at its catalog entry.
fn matches(alias: &Value, target: &Target) -> bool {
    if field(alias, "provider") != target.provider {
        return false;
    }
    match &target.untagged {
        None => alias["catalogModelId"].as_str() == target.tagged_as.as_deref(),
        Some((api, model)) => field(alias, "apiProvider") == api && field(alias, "model") == model,
    }
}

/// applyTag: retagging a consolidated row moves every name that fed it, so the row stays whole.
fn apply_tag(aliases: &[Value], target: &Target, id: &str) -> Vec<Value> {
    let remapped = aliases.iter().map(|a| {
        let mut a = a.clone();
        if matches(&a, target) {
            a["catalogModelId"] = json!(id);
        }
        a
    });
    let Some((api, model)) = &target.untagged else { return remapped.collect() };
    let added = json!({"provider": target.provider, "apiProvider": api, "model": model, "catalogModelId": id});
    remapped.filter(|a| !matches(a, target)).chain(std::iter::once(added)).collect()
}

/// clearTag: drops the target's tags, returning its usage to unpriced.
fn clear_tag(aliases: &[Value], target: &Target) -> Vec<Value> {
    aliases.iter().filter(|a| !matches(a, target)).cloned().collect()
}

fn save(s: &mut App, edit: impl Fn(&[Value], &Target) -> Vec<Value>) {
    let Some(target) = s.ui.pages.usage.tags.target.clone() else { return };
    let aliases = s.client_settings.values.get(SETTING).and_then(Value::as_array).cloned().unwrap_or_default();
    s.client_settings.values.insert(SETTING.into(), Value::Array(edit(&aliases, &target)));
    s.client_settings.save();
    close(s);
}

pub fn view(s: &App, t: &T, overlays: &mut Vec<El>) {
    let u = &s.ui.pages.usage.tags;
    let Some(target) = &u.target else { return };
    let description = match (&target.tagged_as, &target.untagged) {
        (Some(id), _) => format!("Currently counted as {id}. Pick a different model, or remove the tag to leave this usage unpriced."),
        (None, Some((api, _))) if !api.is_empty() => format!("Served through {api}. Pick the model it really is, and its rates apply to this usage everywhere."),
        _ => "Pick the model this really is, and its rates apply to this usage everywhere.".to_string(),
    };
    let input = kit::Input::new(&u.query).placeholder("Search models…").on_edit(|s: &mut App| &mut s.ui.pages.usage.tags.query).build(t).key("usage-tag-query");
    let muted = t.c.muted_foreground;
    let note = |line: &str| row().w_full().py(32.0).center().child(text(line.to_string()).font(theme::TEXT_SM).color(muted));
    let mut list = col().key("usage-tag-models").w_full().h(288.0).gap(4.0).pr(12.0).scroll(ScrollSpec { stick_bottom: false });
    list = if u.query.text.trim().is_empty() {
        list.child(note("Search the models.dev catalog by model or provider name."))
    } else if u.models.is_empty() {
        list.child(note(if u.pending.is_some() { "Searching…" } else { "No models match that search." }))
    } else {
        list.children(u.models.iter().map(|model| model_row(t, model, target.tagged_as.as_deref())))
    };
    let mut footer = row().w_full().gap(8.0).justify_end();
    // The way out: a tag a user regrets has to be removable.
    if target.tagged_as.is_some() {
        footer = footer.child(kit::Button::new("Remove tag").variant("ghost").on_press(|s: &mut App| save(s, clear_tag)).build(t));
    }
    footer = footer.child(kit::Button::new("Cancel").variant("ghost").on_press(close).build(t));
    kit::Dialog::new("usage-tag", format!("Tag {}", target.label))
        .description(description)
        .child(col().w_full().gap(12.0).child(input).child(kit::scroll_area("usage-tag-models", list, t)))
        .child(footer)
        .width(672.0)
        .open(true)
        .on_close(close)
        .build(t, overlays);
}

fn model_row(t: &T, model: &Value, tagged_as: Option<&str>) -> El {
    let id = field(model, "id").to_string();
    let cost = |k: &str| super::usage::usd(model[k].as_f64().unwrap_or(0.0));
    let name = field(model, "modelName").to_string();
    let mut line = row()
        .key(format!("usage-tag-{id}"))
        .w_full()
        .gap(16.0)
        .px(12.0)
        .py(8.0)
        .center_y()
        .radius(radius::MD)
        .hover(t.c.accent)
        .cursor(rune::Cursor::Pointer)
        .role("button")
        .label(name.clone())
        .child(
            col()
                .grow(1.0)
                .shrink()
                .min_w(0.0)
                .child(text(name).font(theme::TEXT_SM).color(t.c.foreground).truncate())
                .child(text(format!("{} · {id}", field(model, "providerName"))).font(theme::TEXT_XS).color(t.c.muted_foreground).truncate()),
        )
        .child(
            text(format!("{} in / {} out per 1M", cost("inputCostPerMillion"), cost("outputCostPerMillion")))
                .font(theme::TEXT_XS.features("'tnum' 1"))
                .color(t.c.muted_foreground),
        );
    if tagged_as == Some(id.as_str()) {
        line = line.fill(t.c.accent.alpha_mul(0.5));
    }
    line.on_press(move |s: &mut App| save(s, |aliases, target| apply_tag(aliases, target, &id)))
}

/// The `usage-tag-dialog` scene: the unpriced Kimi row mid-search, from `usage.json`'s
/// `tagModels` recording.
pub(super) fn scene(s: &mut App) {
    open(
        s,
        Target { provider: "opencode".into(), label: "kimi-k3".into(), untagged: Some(("moonshot".into(), "kimi-k3".into())), tagged_as: None },
    );
    let u = ui(s);
    u.query.replace_text("kimi");
    u.asked = u.query.text.clone();
    u.models = super::fixture("usage.json").and_then(|f| f["tagModels"].as_array().cloned()).unwrap_or_default();
}

#[cfg(test)]
mod tests {
    use super::*;

    const OPUS: &str = "anthropic/claude-opus-5";
    const SONNET: &str = "anthropic/claude-sonnet-5";

    fn alias(provider: &str, api: &str, model: &str, id: &str) -> Value {
        json!({"provider": provider, "apiProvider": api, "model": model, "catalogModelId": id})
    }

    fn untagged(api: &str, model: &str) -> Target {
        Target { provider: "pi".into(), label: model.into(), untagged: Some((api.into(), model.into())), tagged_as: None }
    }

    fn tagged(id: &str) -> Target {
        Target { provider: "pi".into(), label: id.into(), untagged: None, tagged_as: Some(id.into()) }
    }

    #[test]
    fn tagging_replaces_one_names_tag_and_leaves_other_gateways_and_harnesses_alone() {
        let opus = alias("pi", "agentrouter", "claude-opus-5", OPUS);
        let target = untagged("agentrouter", "claude-opus-5");
        assert_eq!(apply_tag(&[], &target, OPUS), [opus.clone()]);
        assert_eq!(apply_tag(&[opus.clone()], &target, SONNET), [alias("pi", "agentrouter", "claude-opus-5", SONNET)]);
        let reseller = alias("pi", "tokenrouter", "claude-opus-5", OPUS);
        assert_eq!(apply_tag(&[opus.clone(), reseller.clone()], &target, SONNET), [reseller, alias("pi", "agentrouter", "claude-opus-5", SONNET)]);
        let claude = alias("claude", "agentrouter", "claude-opus-5", OPUS);
        assert_eq!(apply_tag(&[claude.clone()], &target, SONNET), [claude, alias("pi", "agentrouter", "claude-opus-5", SONNET)]);
    }

    #[test]
    fn a_consolidated_row_retags_and_clears_every_name_feeding_it() {
        let names = [alias("pi", "agentrouter", "claude-opus-5", OPUS), alias("pi", "tokenrouter", "opus-5-latest", OPUS)];
        assert!(apply_tag(&names, &tagged(OPUS), SONNET).iter().all(|a| a["catalogModelId"] == SONNET));
        assert!(clear_tag(&names, &tagged(OPUS)).is_empty());
        let kept = alias("pi", "agentrouter", "sonnet-5-latest", SONNET);
        assert_eq!(clear_tag(&[names[0].clone(), kept.clone()], &tagged(OPUS)), [kept.clone()]);
        assert_eq!(clear_tag(&[names[0].clone(), kept.clone()], &untagged("agentrouter", "claude-opus-5")), [kept]);
    }
}
