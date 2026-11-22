//! Web `CustomModelEditor` and `customModelEditor.logic`: a saved custom model's display name and
//! the options the composer offers for it.
use super::small_input;
use crate::kit;
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, TextEdit, col, row, text};
use serde_json::{Map, Value, json};

struct Preset {
    id: &'static str,
    label: &'static str,
    boolean: bool,
    /// `(id, label, isDefault)`.
    choices: &'static [(&'static str, &'static str, bool)],
}

const EFFORT: &[(&str, &str, bool)] = &[("low", "Low", false), ("medium", "Medium", true), ("high", "High", false), ("xhigh", "Extra High", false)];
const CODEX: &[Preset] = &[
    Preset { id: "reasoningEffort", label: "Reasoning", boolean: false, choices: EFFORT },
    Preset { id: "serviceTier", label: "Speed", boolean: false, choices: &[("default", "Standard", true), ("fast", "Fast", false)] },
];
const CLAUDE: &[Preset] = &[
    Preset {
        id: "effort",
        label: "Reasoning",
        boolean: false,
        choices: &[("low", "Low", false), ("medium", "Medium", false), ("high", "High", true), ("xhigh", "Extra High", false), ("max", "Max", false)],
    },
    Preset { id: "fastMode", label: "Fast Mode", boolean: true, choices: &[] },
    Preset { id: "thinking", label: "Thinking", boolean: true, choices: &[] },
];
const CURSOR: &[Preset] = &[
    Preset { id: "reasoning", label: "Reasoning", boolean: false, choices: EFFORT },
    Preset { id: "fastMode", label: "Fast Mode", boolean: true, choices: &[] },
    Preset { id: "thinking", label: "Thinking", boolean: true, choices: &[] },
];
const GROK: &[Preset] = &[Preset { id: "reasoningEffort", label: "Reasoning", boolean: false, choices: EFFORT }];
const PI: &[Preset] = &[Preset {
    id: "thinking",
    label: "Thinking",
    boolean: false,
    choices: &[
        ("off", "Off", false),
        ("minimal", "Minimal", false),
        ("low", "Low", false),
        ("medium", "Medium", true),
        ("high", "High", false),
        ("xhigh", "Extra High", false),
        ("max", "Max", false),
    ],
}];
const OPENCODE: &[Preset] = &[
    Preset { id: "variant", label: "Reasoning", boolean: false, choices: EFFORT },
    Preset { id: "agent", label: "Agent", boolean: false, choices: &[("build", "Build", true), ("plan", "Plan", false)] },
];

/// `DESCRIPTOR_PRESETS_BY_KIND`.
fn presets(driver: &str) -> &'static [Preset] {
    match driver {
        "codex" => CODEX,
        "claudeAgent" => CLAUDE,
        "cursor" => CURSOR,
        "grok" => GROK,
        "pi" => PI,
        "opencode" => OPENCODE,
        _ => &[],
    }
}

#[derive(Clone, Default)]
struct Choice {
    id: TextEdit,
    label: TextEdit,
    default: bool,
    description: Option<String>,
}

#[derive(Clone, Default)]
struct Descriptor {
    boolean: bool,
    id: TextEdit,
    label: TextEdit,
    choices: Vec<Choice>,
    current: Option<bool>,
    description: Option<String>,
}

pub struct Draft {
    pub slug: String,
    name: TextEdit,
    descriptors: Vec<Descriptor>,
    error: Option<String>,
    /// Takes keystrokes aimed at a row removed since the frame was built.
    scratch: TextEdit,
    /// The option whose preset list is open.
    preset_open: Option<usize>,
    /// Web's "Copy from…" select is open.
    copy_open: bool,
}

fn from_preset(preset: &Preset) -> Descriptor {
    Descriptor {
        boolean: preset.boolean,
        id: TextEdit::new(preset.id),
        label: TextEdit::new(preset.label),
        choices: preset.choices.iter().map(|&(id, label, default)| Choice { id: TextEdit::new(id), label: TextEdit::new(label), default, description: None }).collect(),
        ..Descriptor::default()
    }
}

/// `descriptorToEditor`: prompt-injected choices need runtime profiles custom entries lack.
fn from_descriptor(d: &Value) -> Descriptor {
    let boolean = d["type"] == "boolean";
    let injected = |o: &&Value| d["promptInjectedValues"].as_array().is_some_and(|a| a.contains(&o["id"]));
    let options: Vec<&Value> = if boolean { Vec::new() } else { d["options"].as_array().into_iter().flatten().filter(|o| !injected(o)).collect() };
    let default = options
        .iter()
        .position(|o| d["currentValue"].is_string() && o["id"] == d["currentValue"])
        .or_else(|| options.iter().position(|o| o["isDefault"] == true));
    let text_of = |v: &Value| v.as_str().map(str::to_owned);
    Descriptor {
        boolean,
        id: TextEdit::new(d["id"].as_str().unwrap_or("")),
        label: TextEdit::new(d["label"].as_str().unwrap_or("")),
        choices: options
            .iter()
            .enumerate()
            .map(|(i, o)| Choice {
                id: TextEdit::new(o["id"].as_str().unwrap_or("")),
                label: TextEdit::new(o["label"].as_str().unwrap_or("")),
                default: default == Some(i),
                description: text_of(&o["description"]),
            })
            .collect(),
        current: if boolean { d["currentValue"].as_bool() } else { None },
        description: text_of(&d["description"]),
    }
}

/// `draftFromDefinition`, from a raw `customModels` entry (a slug or an object).
pub fn draft_from_entry(entry: &Value) -> Draft {
    let slug = entry.as_str().or(entry["slug"].as_str()).unwrap_or("").trim().to_owned();
    let name = entry["name"].as_str().map(str::trim).filter(|n| !n.is_empty() && *n != slug).unwrap_or("").to_owned();
    Draft {
        descriptors: entry["capabilities"]["optionDescriptors"].as_array().into_iter().flatten().map(from_descriptor).collect(),
        name: TextEdit::new(name),
        slug,
        error: None,
        scratch: TextEdit::default(),
        preset_open: None,
        copy_open: false,
    }
}

/// `descriptorsFromCapabilities`: Claude context choices need runtime suffixes custom entries lack.
fn from_capabilities(capabilities: &Value, driver: &str) -> Vec<Descriptor> {
    capabilities["optionDescriptors"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|d| driver != "claudeAgent" || d["id"] != "contextWindow")
        .map(from_descriptor)
        .collect()
}

/// `validateDraft`.
pub fn validate(draft: &Draft) -> Option<String> {
    let mut seen: Vec<&str> = Vec::new();
    for (index, d) in draft.descriptors.iter().enumerate() {
        let position = format!("Option {}", index + 1);
        let id = d.id.text.trim();
        if id.is_empty() {
            return Some(format!("{position} needs an id."));
        }
        if seen.contains(&id) {
            return Some(format!("{position}: id \"{id}\" is used twice."));
        }
        seen.push(id);
        if d.label.text.trim().is_empty() {
            return Some(format!("{position} needs a label."));
        }
        if d.boolean {
            continue;
        }
        if d.choices.is_empty() {
            return Some(format!("{position} needs at least one choice."));
        }
        let mut choices: Vec<&str> = Vec::new();
        for choice in &d.choices {
            let choice_id = choice.id.text.trim();
            if choice_id.is_empty() {
                return Some(format!("{position} has a choice without a value."));
            }
            if choices.contains(&choice_id) {
                return Some(format!("{position}: choice \"{choice_id}\" is used twice."));
            }
            choices.push(choice_id);
        }
    }
    None
}

/// `toCustomModelSetting(definitionFromDraft(draft))`: a bare slug when nothing else is set.
pub fn setting(draft: &Draft) -> Value {
    let descriptors: Vec<Value> = draft
        .descriptors
        .iter()
        .map(|d| {
            let mut out = Map::new();
            out.insert("id".into(), json!(d.id.text.trim()));
            out.insert("label".into(), json!(d.label.text.trim()));
            out.insert("type".into(), json!(if d.boolean { "boolean" } else { "select" }));
            if let Some(description) = &d.description {
                out.insert("description".into(), json!(description));
            }
            if d.boolean {
                if let Some(current) = d.current {
                    out.insert("currentValue".into(), json!(current));
                }
                return Value::Object(out);
            }
            let options: Vec<Value> = d
                .choices
                .iter()
                .map(|c| {
                    let (id, label) = (c.id.text.trim(), c.label.text.trim());
                    let mut option = json!({"id": id, "label": if label.is_empty() { id } else { label }});
                    if let Some(description) = &c.description {
                        option["description"] = json!(description);
                    }
                    if c.default {
                        option["isDefault"] = json!(true);
                    }
                    option
                })
                .collect();
            out.insert("options".into(), json!(options));
            if let Some(current) = d.choices.iter().find(|c| c.default).map(|c| c.id.text.trim()).filter(|id| !id.is_empty()) {
                out.insert("currentValue".into(), json!(current));
            }
            Value::Object(out)
        })
        .collect();
    let name = draft.name.text.trim();
    let named = !name.is_empty() && name != draft.slug;
    if !named && descriptors.is_empty() {
        return json!(draft.slug);
    }
    let mut out = json!({"slug": draft.slug});
    if named {
        out["name"] = json!(name);
    }
    if !descriptors.is_empty() {
        out["capabilities"] = json!({"optionDescriptors": descriptors});
    }
    out
}

fn same_slug(raw: &Value, slug: &str) -> bool {
    raw.as_str().or(raw["slug"].as_str()).map(str::trim) == Some(slug)
}

pub fn scene_open_preset(s: &mut App, option: usize) {
    if let Some(draft) = s.ui.settings.providers_ui.editing.as_mut() {
        draft.preset_open = Some(option);
    }
}

/// The pencil: opens the editor on a custom model, or closes it when it is already open there.
pub fn toggle(s: &mut App, id: &str, slug: &str) {
    if s.ui.settings.providers_ui.editing.as_ref().is_some_and(|d| d.slug == slug) {
        s.ui.settings.providers_ui.editing = None;
        return;
    }
    let driver = super::driver_of(s, id);
    let instance = super::instance_of(s, id, &driver);
    let entry = instance["config"]["customModels"].as_array().into_iter().flatten().find(|raw| same_slug(raw, slug)).cloned();
    s.ui.settings.providers_ui.editing = Some(draft_from_entry(&entry.unwrap_or_else(|| json!(slug))));
}

fn save(s: &mut App, id: &str) {
    let ui = &mut s.ui.settings.providers_ui;
    let Some(draft) = ui.editing.as_mut() else { return };
    if let Some(problem) = validate(draft) {
        draft.error = Some(problem);
        return;
    }
    let (slug, next_entry) = (draft.slug.clone(), setting(draft));
    ui.editing = None;
    let driver = super::driver_of(s, id);
    let mut next = super::instance_of(s, id, &driver);
    let Some(saved) = next.get_mut("config").and_then(|c| c.get_mut("customModels")).and_then(Value::as_array_mut) else { return };
    for raw in saved.iter_mut().filter(|raw| same_slug(raw, &slug)) {
        *raw = next_entry.clone();
    }
    super::upsert(s, id, next, json!({}));
}

#[derive(Clone, Copy)]
enum Slot {
    Name,
    Id(usize),
    Label(usize),
    ChoiceId(usize, usize),
    ChoiceLabel(usize, usize),
}

fn slot(s: &mut App, slot: Slot) -> &mut TextEdit {
    let ui = &mut s.ui.settings.providers_ui;
    let Some(d) = ui.editing.as_mut() else { return &mut ui.custom };
    let exists = match slot {
        Slot::Name => true,
        Slot::Id(i) | Slot::Label(i) => i < d.descriptors.len(),
        Slot::ChoiceId(i, j) | Slot::ChoiceLabel(i, j) => d.descriptors.get(i).is_some_and(|o| j < o.choices.len()),
    };
    if !exists {
        return &mut d.scratch;
    }
    match slot {
        Slot::Name => &mut d.name,
        Slot::Id(i) => &mut d.descriptors[i].id,
        Slot::Label(i) => &mut d.descriptors[i].label,
        Slot::ChoiceId(i, j) => &mut d.descriptors[i].choices[j].id,
        Slot::ChoiceLabel(i, j) => &mut d.descriptors[i].choices[j].label,
    }
}

fn edit(s: &mut App, f: impl FnOnce(&mut Draft)) {
    if let Some(d) = s.ui.settings.providers_ui.editing.as_mut() {
        d.error = None;
        f(d);
    }
}

fn edit_option(s: &mut App, i: usize, f: impl FnOnce(&mut Descriptor)) {
    edit(s, |d| {
        if let Some(o) = d.descriptors.get_mut(i) {
            f(o);
        }
    });
}

/// Web's `applyPresetId`: a preset replaces the option outright; "Custom…" clears its id.
fn apply_preset(o: &mut Descriptor, preset: Option<&Preset>) {
    *o = match preset {
        Some(preset) => from_preset(preset),
        None => Descriptor { id: TextEdit::default(), ..o.clone() },
    };
}

/// Closes the open preset list, else the editor, as Escape does.
pub fn escape(s: &mut App) -> bool {
    let editing = &mut s.ui.settings.providers_ui.editing;
    if let Some(d) = editing.as_mut().filter(|d| d.preset_open.is_some() || d.copy_open) {
        (d.preset_open, d.copy_open) = (None, false);
        return true;
    }
    editing.take().is_some()
}

fn micro_remove(t: &T, label: String, press: impl Fn(&mut App) + 'static) -> El {
    kit::Button::new(label).variant("ghost-muted").size("icon-micro").icon(&icons::X).on_press(press).build(t)
}

/// The editor under a custom model's row, while its pencil is on.
pub fn editor(s: &App, t: &T, id: &str, driver: &str, live: Option<&Value>, slug: &str) -> Option<El> {
    let draft = s.ui.settings.providers_ui.editing.as_ref().filter(|d| d.slug == slug)?;
    let available = presets(driver);
    let muted = t.c.muted_foreground;
    let mut body = col()
        .key(format!("custom-model-editor-{slug}"))
        .mx(8.0)
        .mt(4.0)
        .mb(8.0)
        .px(12.0)
        .py(12.0)
        .gap(12.0)
        .radius(theme::radius::MD)
        .border(1.0, t.c.border)
        .fill(t.c.muted.alpha_mul(0.2))
        .child(
            col().gap(4.0).child(text("Display name").font(theme::TEXT_XS).color(muted)).child(
                small_input(&draft.name, &format!("custom-model-{slug}-name"), slug, t, false)
                    .w(288.0)
                    .label("Display name")
                    .on_edit(|s: &mut App| slot(s, Slot::Name)),
            ),
        );

    let mut heading = row().w_full().gap(8.0).center_y().flex_wrap().child(text("Options shown in the composer").font(theme::TEXT_XS).color(muted).grow(1.0));
    let candidates: Vec<&Value> = live
        .and_then(|p| p["models"].as_array())
        .into_iter()
        .flatten()
        .filter(|m| m["isCustom"] != true && m["capabilities"]["optionDescriptors"].as_array().is_some_and(|d| !d.is_empty()))
        .collect();
    let mut copy_list = None;
    if !candidates.is_empty() {
        heading = heading.child(
            kit::Button::new("Copy from…")
                .variant("outline")
                .size("xs")
                .icon(&icons::CHEVRON_DOWN)
                .on_press(|s: &mut App| edit(s, |d| d.copy_open ^= true))
                .build(t)
                .w(176.0)
                .label("Copy options from a built-in model")
                .expanded(draft.copy_open),
        );
        if draft.copy_open {
            let mut list = col().w(176.0).self_end().gap(2.0).padding(4.0).radius(8.0).border(1.0, t.c.border).fill(t.c.popover).role("listbox").label("Copy options from a built-in model");
            for model in candidates {
                let (capabilities, owned_driver) = (model["capabilities"].clone(), driver.to_owned());
                let name = model["name"].as_str().or(model["slug"].as_str()).unwrap_or("").to_owned();
                list = list.child(
                    row()
                        .w_full()
                        .px(8.0)
                        .py(6.0)
                        .radius(6.0)
                        .role("option")
                        .cursor(rune::Cursor::Pointer)
                        .hover(t.c.muted)
                        .on_press(move |s: &mut App| {
                            edit(s, |d| {
                                d.descriptors = from_capabilities(&capabilities, &owned_driver);
                                d.copy_open = false;
                            })
                        })
                        .child(text(name).font(theme::TEXT_SM).color(t.c.foreground).truncate()),
                );
            }
            copy_list = Some(list);
        }
    }
    let mut options = col().w_full().gap(8.0).child(heading).children(copy_list);
    if draft.descriptors.is_empty() {
        options = options.child(text("No custom options. The composer uses the provider's default options.").font(theme::TEXT_XS).color(muted.alpha_mul(0.7)).wrap());
    }
    for (i, d) in draft.descriptors.iter().enumerate() {
        let mut kinds = row().gap(2.0).center_y().role("radiogroup").label("Option type");
        for (boolean, label) in [(false, "Choices"), (true, "Toggle")] {
            kinds = kinds.child(
                kit::Button::new(label)
                    .variant(if d.boolean == boolean { "secondary" } else { "ghost-muted" })
                    .size("xs")
                    .on_press(move |s: &mut App| edit_option(s, i, |o| o.boolean = boolean))
                    .build(t)
                    .role("radio")
                    .checked(d.boolean == boolean),
            );
        }
        let header = row()
            .w_full()
            .gap(8.0)
            .center_y()
            .flex_wrap()
            .child(text(format!("Option {}", i + 1)).font(theme::TEXT_2XS).color(muted).w(56.0));
        let preset = available.iter().position(|p| p.id == d.id.text);
        let header = if available.is_empty() {
            header
        } else {
            header.child(
                kit::Button::new(preset.map_or("Custom…", |p| available[p].id))
                    .variant("outline")
                    .size("xs")
                    .icon(&icons::CHEVRON_DOWN)
                    .on_press(move |s: &mut App| edit(s, |d| d.preset_open = if d.preset_open == Some(i) { None } else { Some(i) }))
                    .build(t)
                    .w(160.0)
                    .label("Option id")
                    .expanded(draft.preset_open == Some(i)),
            )
        };
        let header = if preset.is_none() {
            header.child(small_input(&d.id, &format!("custom-model-{slug}-option-{i}-id"), "optionId", t, true).w(144.0).label("Option id").on_edit(move |s: &mut App| slot(s, Slot::Id(i))))
        } else {
            header
        };
        let header = header
            .child(
                small_input(&d.label, &format!("custom-model-{slug}-option-{i}-label"), "Label", t, false)
                    .grow(1.0)
                    .min_w(96.0)
                    .label("Option label")
                    .on_edit(move |s: &mut App| slot(s, Slot::Label(i))),
            )
            .child(kinds)
            .child(micro_remove(t, format!("Remove option {}", i + 1), move |s: &mut App| {
                edit(s, |d| {
                    if i < d.descriptors.len() {
                        d.descriptors.remove(i);
                    }
                })
            }));
        let mut block = col().w_full().px(10.0).py(10.0).gap(8.0).radius(theme::radius::MD).border(1.0, t.c.border.alpha_mul(0.6)).fill(t.c.background.alpha_mul(0.4)).child(header);
        if draft.preset_open == Some(i) {
            let mut list = col().w(240.0).ml(64.0).gap(2.0).padding(4.0).radius(8.0).border(1.0, t.c.border).fill(t.c.popover).role("listbox").label("Option id");
            for p in (0..available.len()).map(Some).chain([None]) {
                let chosen = p == preset;
                let owned = driver.to_owned();
                let mut item = row()
                    .w_full()
                    .px(8.0)
                    .py(6.0)
                    .gap(8.0)
                    .center_y()
                    .radius(6.0)
                    .role("option")
                    .checked(chosen)
                    .cursor(rune::Cursor::Pointer)
                    .hover(t.c.muted)
                    .on_press(move |s: &mut App| {
                        let preset = p.map(|p| &presets(&owned)[p]);
                        edit_option(s, i, |o| apply_preset(o, preset));
                        edit(s, |d| d.preset_open = None);
                    });
                item = match p {
                    Some(p) => item
                        .child(text(available[p].id).font(theme::MONO_SM).color(t.c.foreground))
                        .child(text(available[p].label).font(theme::TEXT_XS).color(muted).grow(1.0)),
                    None => item.child(text("Custom…").font(theme::TEXT_SM).color(t.c.foreground).grow(1.0)),
                };
                if chosen {
                    item = item.child(rune::icon(&icons::CHECK, 14.0).color(t.c.foreground));
                }
                list = list.child(item);
            }
            block = block.child(list);
        }
        if !d.boolean {
            let mut choices = col().w_full().pl(64.0).gap(6.0);
            for (j, c) in d.choices.iter().enumerate() {
                choices = choices.child(
                    row()
                        .w_full()
                        .gap(8.0)
                        .center_y()
                        .child(
                            small_input(&c.id, &format!("custom-model-{slug}-option-{i}-choice-{j}-id"), "value", t, true)
                                .w(112.0)
                                .label("Choice value")
                                .on_edit(move |s: &mut App| slot(s, Slot::ChoiceId(i, j))),
                        )
                        .child(
                            small_input(&c.label, &format!("custom-model-{slug}-option-{i}-choice-{j}-label"), "Label", t, false)
                                .grow(1.0)
                                .min_w(0.0)
                                .label("Choice label")
                                .on_edit(move |s: &mut App| slot(s, Slot::ChoiceLabel(i, j))),
                        )
                        .child(
                            row()
                                .gap(6.0)
                                .center_y()
                                .child(
                                    kit::Switch::new(c.default)
                                        .label("Default choice")
                                        .hide_label()
                                        .size("sm")
                                        .on_toggle(move |s: &mut App| {
                                            edit_option(s, i, |o| {
                                                let on = !o.choices.get(j).is_some_and(|c| c.default);
                                                for (k, choice) in o.choices.iter_mut().enumerate() {
                                                    choice.default = on && k == j;
                                                }
                                            })
                                        })
                                        .build(&kit::theme(t)),
                                )
                                .child(text("Default").font(theme::TEXT_2XS).color(muted)),
                        )
                        .child(micro_remove(t, "Remove choice".into(), move |s: &mut App| {
                            edit_option(s, i, |o| {
                                if j < o.choices.len() {
                                    o.choices.remove(j);
                                }
                            })
                        })),
                );
            }
            choices = choices.child(
                kit::Button::new("Add choice")
                    .variant("ghost-muted")
                    .size("xs")
                    .icon(&icons::PLUS)
                    .on_press(move |s: &mut App| edit_option(s, i, |o| o.choices.push(Choice::default())))
                    .build(t)
                    .self_start(),
            );
            block = block.child(choices);
        }
        options = options.child(block);
    }
    let mut adders = row().gap(4.0).center_y().flex_wrap();
    for (p, preset) in available.iter().enumerate() {
        if draft.descriptors.iter().any(|d| d.id.text == preset.id) {
            continue;
        }
        let owned = driver.to_owned();
        adders = adders.child(
            kit::Button::new(preset.label)
                .variant("ghost-muted")
                .size("xs")
                .icon(&icons::PLUS)
                .on_press(move |s: &mut App| {
                    let descriptor = from_preset(&presets(&owned)[p]);
                    edit(s, |d| d.descriptors.push(descriptor));
                })
                .build(t),
        );
    }
    adders = adders.child(
        kit::Button::new("Custom option")
            .variant("ghost-muted")
            .size("xs")
            .icon(&icons::PLUS)
            .on_press(|s: &mut App| edit(s, |d| d.descriptors.push(Descriptor::default())))
            .build(t),
    );
    body = body.child(options.child(adders));
    if let Some(error) = &draft.error {
        body = body.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap().role("alert"));
    }
    let owned = id.to_owned();
    Some(
        body.child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Save").variant("outline").size("sm").on_press(move |s: &mut App| save(s, &owned)).build(t))
                .child(kit::Button::new("Cancel").variant("ghost").size("sm").on_press(|s: &mut App| s.ui.settings.providers_ui.editing = None).build(t)),
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn draft(entry: Value) -> Draft {
        draft_from_entry(&entry)
    }

    #[test]
    fn drafts_round_trip_saved_entries_like_web() {
        assert_eq!(setting(&draft(json!(" my-model "))), json!("my-model"));
        // A name equal to the slug is no name at all.
        assert_eq!(setting(&draft(json!({"slug": "m", "name": "m"}))), json!("m"));
        assert_eq!(setting(&draft(json!({"slug": "m", "name": "Mine"}))), json!({"slug": "m", "name": "Mine"}));

        let entry = json!({"slug": "m", "capabilities": {"optionDescriptors": [
            {"id": "effort", "label": "Reasoning", "type": "select", "currentValue": "high", "promptInjectedValues": ["ultra"],
             "options": [{"id": "low", "label": "Low", "isDefault": true}, {"id": "high", "label": "High"}, {"id": "ultra", "label": "Ultra"}]},
            {"id": "fastMode", "label": "Fast", "type": "boolean", "currentValue": true}
        ]}});
        let mut d = draft(entry);
        assert_eq!(d.descriptors[0].choices.len(), 2, "prompt-injected choices are dropped");
        assert!(d.descriptors[0].choices[1].default, "currentValue picks the default over isDefault");
        d.descriptors[0].choices[0].label = TextEdit::new(" ");
        assert_eq!(
            setting(&d),
            json!({"slug": "m", "capabilities": {"optionDescriptors": [
                {"id": "effort", "label": "Reasoning", "type": "select", "currentValue": "high",
                 "options": [{"id": "low", "label": "low"}, {"id": "high", "label": "High", "isDefault": true}]},
                {"id": "fastMode", "label": "Fast", "type": "boolean", "currentValue": true}
            ]}})
        );
        let copied = from_capabilities(&json!({"optionDescriptors": [{"id": "contextWindow", "label": "Context", "type": "select", "options": []}]}), "claudeAgent");
        assert!(copied.is_empty(), "Claude context choices are not copied");
        assert_eq!(from_preset(&CODEX[1]).choices.iter().filter(|c| c.default).count(), 1);
    }

    #[test]
    fn a_preset_replaces_the_option_and_custom_clears_its_id() {
        let mut option = from_preset(&CODEX[1]);
        option.label = TextEdit::new("Renamed");
        apply_preset(&mut option, Some(&CODEX[0]));
        assert_eq!((option.id.text.as_str(), option.label.text.as_str(), option.choices.len()), ("reasoningEffort", "Reasoning", 4));
        apply_preset(&mut option, None);
        assert_eq!((option.id.text.as_str(), option.label.text.as_str(), option.choices.len()), ("", "Reasoning", 4), "FAILED: Custom keeps the label and choices");
    }

    #[test]
    fn validation_names_the_first_problem() {
        let mut d = draft(json!("m"));
        assert_eq!(validate(&d), None);
        d.descriptors.push(Descriptor::default());
        assert_eq!(validate(&d).as_deref(), Some("Option 1 needs an id."));
        d.descriptors[0].id = TextEdit::new("effort");
        assert_eq!(validate(&d).as_deref(), Some("Option 1 needs a label."));
        d.descriptors[0].label = TextEdit::new("Effort");
        assert_eq!(validate(&d).as_deref(), Some("Option 1 needs at least one choice."));
        d.descriptors[0].choices = vec![Choice::default()];
        assert_eq!(validate(&d).as_deref(), Some("Option 1 has a choice without a value."));
        d.descriptors[0].choices = vec![Choice { id: TextEdit::new("a"), ..Choice::default() }, Choice { id: TextEdit::new(" a "), ..Choice::default() }];
        assert_eq!(validate(&d).as_deref(), Some("Option 1: choice \"a\" is used twice."));
        d.descriptors[0].boolean = true;
        d.descriptors.push(Descriptor { boolean: true, id: TextEdit::new("effort"), label: TextEdit::new("x"), ..Descriptor::default() });
        assert_eq!(validate(&d).as_deref(), Some("Option 2: id \"effort\" is used twice."));
    }
}
