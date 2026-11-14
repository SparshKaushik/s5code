use super::{update, value};
use crate::theme::{self, AlphaMul, T};
use crate::{App, icons};
use rune::{El, col, el, row, text};
use serde_json::{Value, json};

pub fn button(label: &str, t: &T) -> El {
    crate::kit::Button::new(label)
        .variant("outline")
        .size("compact")
        .build(t)
}

/// `on_press` when `allowed`; otherwise dimmed like a disabled kit button and unreachable.
pub fn press_if(control: El, allowed: bool, f: impl Fn(&mut App) + 'static) -> El {
    if allowed { control.on_press(f) } else { control.disabled(true).inert().opacity(0.64) }
}

pub fn switch(s: &App, t: &T, key: &str, label: &str) -> El {
    let current = value(s, key);
    let on = if key == "sidebarProjectGroupingMode" {
        current.as_str() != Some("separate")
    } else {
        current.as_bool().unwrap_or(false)
    };
    let owned = key.to_owned();
    crate::kit::Switch::new(on)
        .label(label)
        .hide_label()
        .on_toggle(move |s: &mut App| {
            let key = owned.as_str();
            let v = if key == "sidebarProjectGroupingMode" {
                json!(toggle_grouping(value(s, key).as_str().unwrap_or("repository"), on))
            } else {
                json!(!on)
            };
            update(s, key, v);
            if key == "boringSidebarEnabled" && !on {
                update(s, "legacySidebarEnabled", json!(false));
            }
        })
        .build(&crate::kit::theme(t))
        .key(format!("setting-{key}"))
}

/// `projectGroupingModeFromToggle`: grouping switched back on restores the mode it was switched
/// off from, which the web keeps in local storage.
fn grouping_from_toggle(enabled: bool, last: &str) -> &'static str {
    match (enabled, last) {
        (false, _) => "separate",
        (true, "repository_path") => "repository_path",
        _ => "repository",
    }
}

fn toggle_grouping(current: &str, on: bool) -> &'static str {
    let path = (!cfg!(test)).then(|| crate::settings::client::sibling_path("last-project-grouping-mode"));
    if on && current != "separate" && let Some(p) = &path {
        let _ = p.parent().map(std::fs::create_dir_all);
        let _ = std::fs::write(p, current);
    }
    let last = path.and_then(|p| std::fs::read_to_string(p).ok()).unwrap_or_default();
    grouping_from_toggle(!on, last.trim())
}

pub fn select_width(key: &str) -> f32 {
    match key {
        "notificationMode" | "sourceControlWritingStyle.mode" => 224.0,
        "sidebarProjectSortOrder" => 176.0,
        // Web `w-auto`: the trigger fits the current label.
        "sendShortcut" | "followUpBehavior" => 0.0,
        k if k.starts_with("snooze::when::") => 128.0,
        k if k.starts_with("snooze::unit::") => 112.0,
        // Web `w-22`: the font size trigger beside its family field.
        k if k.starts_with("fontSize") => 88.0,
        // Web `w-64`: a source control host's account picker.
        k if k.starts_with("schost::") => 256.0,
        _ => 160.0,
    }
}

fn current_label(s: &App, key: &str, options: &[(Value, String)]) -> String {
    let current = match value(s, key) {
        // Unset values resolve to their built-in defaults (`resolveProjectSettings`).
        Value::Null if key == "defaultThreadEnvMode" => json!("local"),
        Value::Null if key == "worktreeSubmodules" => json!("recursive"),
        current => current,
    };
    options
        .iter()
        .find(|(v, _)| *v == current)
        .map(|(_, l)| l.clone())
        .unwrap_or_else(|| current.as_str().unwrap_or("Default").to_owned())
}

fn labels(options: &[(Value, String)]) -> Vec<String> {
    options.iter().map(|(_, l)| l.clone()).collect()
}

/// The setting's `kit::Select` trigger; its list mounts in the settings overlay (`select_menu`).
pub fn select(
    s: &App,
    t: &T,
    key: &str,
    title: &str,
    options: &[(Value, String)],
    width: f32,
) -> El {
    let open = s.ui.settings.open_select.as_deref() == Some(key);
    let owned = key.to_owned();
    let choices = options.to_vec();
    // The kit wraps its trigger in a `self_start` row; this row lets the setting row centre it.
    let mut select = crate::kit::Select::new(format!("setting-{key}"), &labels(options)).current(Some(&current_label(s, key, options)));
    match (key, value(s, key).as_str()) {
        // `DiffColorSchemeRow`: the scheme's two dots, and the name without "(default)".
        ("diffColorScheme", scheme) => {
            let blue = scheme == Some("blue-orange");
            let dot = |c: rune::Color| el().size(8.0, 8.0).radius(4.0).fill(c);
            let dots = row().gap(4.0).child(dot(if blue { t.c.diff_addition } else { t.c.diff_deletion })).child(dot(if blue { t.c.diff_deletion } else { t.c.diff_addition }));
            select = select.leading(dots, 20.0).display(if blue { "Blue & orange" } else { "Red & green" });
        }
        ("defaultRuntimeMode", Some(mode)) => {
            if let Some((.., glyph)) = crate::chat::composer::RUNTIME_MODES.iter().find(|m| m.0 == mode) {
                select = select.leading(rune::icon(*glyph, 14.0).color(t.c.muted_foreground), 14.0);
            }
        }
        _ => {}
    }
    row().child(select
        .label(title)
        .width(width)
        .size("sm")
        .on_toggle(move |s: &mut App| {
            s.ui.settings.open_select = if open { None } else { Some(owned.clone()) };
            s.ui.settings.select_options = choices.clone();
        })
        .trigger(&crate::kit::theme(t)))
}

/// The open select's list, anchored to the trigger `select` mounted.
pub fn select_menu(s: &App, t: &T, key: &str, options: &[(Value, String)], width: f32, layer: &mut Vec<El>) {
    let (owned, choices) = (key.to_owned(), options.to_vec());
    let mut select = crate::kit::Select::new(format!("setting-{key}"), &labels(options));
    if key == "defaultRuntimeMode" {
        for (_, label, description, glyph) in crate::chat::composer::RUNTIME_MODES {
            select = select.option_detail(label, glyph, description);
        }
    }
    let _trigger = select
        .current(Some(&current_label(s, key, options)))
        .width(width)
        .size("sm")
        .align_end()
        .open(true)
        .on_choose(move |s: &mut App, label: &str, tx: &mut rune::Tx| {
            if let Some((v, _)) = choices.iter().find(|(_, l)| l == label) {
                if owned == super::fonts::ZOOM_KEY {
                    tx.instant();
                }
                update(s, &owned, v.clone());
            }
            s.ui.settings.open_select = None;
        })
        .on_close(|s: &mut App| s.ui.settings.open_select = None)
        .build(&crate::kit::theme(t), layer);
}

/// Markdown `[label](href)` links in a description: the plain copy and each label's byte range.
fn links(copy: &str) -> (String, Vec<(std::ops::Range<usize>, String)>) {
    let mut plain = String::new();
    let mut links = Vec::new();
    let mut rest = copy;
    while let Some(open) = rest.find('[') {
        let Some((label, after)) = rest[open + 1..].split_once("](") else { break };
        let Some((href, tail)) = after.split_once(')') else { break };
        plain.push_str(&rest[..open]);
        let start = plain.len();
        plain.push_str(label);
        links.push((start..plain.len(), href.to_owned()));
        rest = tail;
    }
    plain.push_str(rest);
    (plain, links)
}

/// Web's follow-up copy names the shortcut that flips queue/steer under the chosen send key.
fn follow_up_description(send_shortcut: Option<&str>, modifier: &str) -> String {
    let flip = match send_shortcut {
        Some("mod-enter-multiline") => {
            format!("Press {modifier} + Enter for single-line prompts or {modifier} + Shift + Enter for multiline prompts to do the opposite for one message.")
        }
        Some("mod-enter") => format!("Press {modifier} + Shift + Enter to do the opposite for one message."),
        _ => format!("Press {modifier} + Enter to do the opposite for one message."),
    };
    format!("Queue follow-ups while the agent runs or steer the current run. {flip}")
}

/// Web `SettingsRow`'s reason a row's control is inert: an administrator's lock, then a
/// connection that may not write settings, then a project scope over an environment-wide key.
fn row_lock(s: &App, key: &str) -> Option<String> {
    let client = s.client_settings.values.contains_key(key) || key == "appearance";
    if client || key.starts_with("project::") {
        return None;
    }
    let environment_wide = s.ui.settings.project.is_some() && !super::scope::project_key(key);
    super::permissions::row_lock(s, key)
        .or_else(|| environment_wide.then(|| "Environment-wide setting. Select an environment to change it.".to_owned()))
}

pub fn setting_row(s: &App, cx: &rune::Cx<App>, t: &T, spec: &Value) -> El {
    let key = spec["key"].as_str().unwrap_or("");
    let title = spec["title"].as_str().unwrap_or(key);
    let dynamic = super::models::description(s, key).or_else(|| {
        if key == "followUpBehavior" {
            let modifier = if cfg!(target_os = "macos") { "⌘" } else { "Ctrl" };
            return Some(follow_up_description(value(s, "sendShortcut").as_str(), modifier));
        }
        let text = match (key, value(s, key).as_str()) {
            ("responseStreamingMode", Some("turn")) => "Text appears once the agent finishes its turn.",
            ("responseStreamingMode", _) => "Each paragraph or code block appears as soon as it is complete.",
            ("sourceControlWritingStyle.mode", Some("conventional_commits")) => {
                "Use Conventional Commit prefixes and keep change request text concise."
            }
            ("sourceControlWritingStyle.mode", Some("custom")) => {
                "Use your instructions for change descriptions and change requests in every project."
            }
            ("sourceControlWritingStyle.mode", _) => {
                "In each project, matches recent change descriptions and change request titles."
            }
            _ => return None,
        };
        Some(text.to_owned())
    });
    let description = dynamic.as_deref().unwrap_or_else(|| spec["description"].as_str().unwrap_or(""));
    let mut heading = row().gap(6.0).center_y().child(
        text(title.to_owned())
            .key(super::title_key(spec["searchTitle"].as_str().unwrap_or(title)))
            .font(theme::SM_MEDIUM)
            .color(t.c.foreground),
    );
    // Web resets the auto-settle day count from its switch row only, and has no reset for the
    // Separate fonts switch; a font row resets its family and size together.
    let size_key = spec["size"].as_str().filter(|_| spec["kind"] == "font").map(str::to_owned);
    let customized = super::scope::customized(s, key) || size_key.as_deref().is_some_and(|k| super::scope::customized(s, k));
    let lock = row_lock(s, key);
    if !matches!(key, "sidebarAutoSettleAfterDays" | "typographyAdvanced") && customized && lock.is_none() {
        let owned = key.to_owned();
        let size_key = size_key.clone();
        heading = heading.child(
            crate::kit::Button::new(format!("Reset {title}"))
                .variant("ghost-muted")
                .size("icon-micro")
                .icon(&icons::UNDO_2)
                .build(t)
                .on_press_tx(move |s: &mut App, tx: &mut rune::Tx| {
                    super::scope::reset(s, &owned);
                    if let Some(size) = &size_key {
                        if size == super::fonts::ZOOM_KEY {
                            tx.instant();
                        }
                        super::scope::reset(s, size);
                    }
                }),
        );
    }
    let mut copy = col()
        .key(format!("copy-{key}"))
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(heading);
    if !description.is_empty() {
        let (plain, links) = links(description);
        let mut body = text(super::balance::description(
            &plain,
            cx.layout(&format!("copy-{key}"))
                .map(|r| r[2])
                .unwrap_or(558.0)
                .min(672.0),
        ))
        .font(theme::TEXT_XS.lh(18.0))
        .color(t.c.muted_foreground)
        .wrap();
        if !links.is_empty() {
            let underline = t.c.muted_foreground.alpha_mul(0.5);
            body = body.spans(
                links
                    .iter()
                    .map(|(r, _)| rune::Span::new(r.start, r.end).underline(underline))
                    .collect::<Vec<_>>(),
            );
            for (range, href) in links {
                body = body.link(
                    rune::Link::new(range, href)
                        .hover(rune::LinkPaint::new().color(t.c.foreground).underline(t.c.foreground)),
                );
            }
            body = body.on_link(|s: &mut App, href: &str, _tx: &mut rune::Tx| match href.strip_prefix("/settings/") {
                Some(page) => s.navigate(crate::Route::Settings(page.into())),
                None => super::auth::open_url(s, href),
            });
        }
        copy = copy.child(body);
    }
    let (ctrl, children) = match spec["kind"].as_str() {
        Some("snooze") => super::snooze::parts(s, t),
        Some("profiles") => super::browser_profiles::parts(s, t),
        Some("textarea") => (el(), Some(textarea(s, t, key, title, spec["placeholder"].as_str().unwrap_or("")))),
        kind => (match kind {
        Some("switch") => row()
            .w(160.0)
            .child(el().grow(1.0))
            .child(switch(s, t, key, title)),
        Some("select") => {
            let options: Vec<_> = spec["options"]
                .as_array()
                .into_iter()
                .flatten()
                .map(|o| (o[0].clone(), o[1].as_str().unwrap_or("").to_owned()))
                .collect();
            select(s, t, key, title, &options, select_width(key))
        }
        Some("slider") => slider(s, t, key, title),
        Some("font") => font(s, t, key, title, spec),
        Some("model") => super::models::control(s, t, key),
        Some("secret") => super::providers::secret_control(s, t, key, title, spec["placeholder"].as_str().unwrap_or("")),
        _ => input(
            s,
            t,
            key,
            spec["width"].as_f64().map_or(208.0, |w| w as f32),
            spec["placeholder"].as_str().unwrap_or(""),
            spec["size"] == "default",
        ),
    }, None),
    };
    // A field that belongs under the control, like web's custom instructions under the mode.
    let area = &spec["textarea"];
    let children = children.or_else(|| {
        (area["key"].is_string() && visible(s, area)).then(|| {
            textarea(
                s,
                t,
                area["key"].as_str().unwrap_or(""),
                area["label"].as_str().unwrap_or(title),
                area["placeholder"].as_str().unwrap_or(""),
            )
        })
    });
    let ctrl = super::permissions::gate(key, ctrl, lock);
    if s.ui.settings.project.is_some() && super::scope::project_key(key) {
        copy = copy.child(
            text(if super::scope::customized(s, key) {
                "Overridden for this project"
            } else {
                "Inherited from environment"
            })
            .font(theme::TEXT_XS)
            .color(t.c.muted_foreground),
        );
    }
    let line = row().w_full().gap(32.0).center_y().child(copy).child(ctrl);
    // `SettingsRow` children sit under the copy and control: `pt-3 pb-1` instead of `py-3`.
    match children {
        Some(children) => col().key(format!("row-{key}")).w_full().px(16.0).pt(12.0).pb(4.0).child(line).child(children),
        None => line.key(format!("row-{key}")).px(16.0).py(12.0),
    }
}

/// Web `FontFamilySettingsRow`'s control: the family field (`w-44`, placeholder the default
/// family) and its `N px` size select.
fn font(s: &App, t: &T, key: &str, title: &str, spec: &Value) -> El {
    let size = spec["size"].as_str().unwrap_or("");
    let (min, max) = (spec["min"].as_u64().unwrap_or(10), spec["max"].as_u64().unwrap_or(20));
    let options: Vec<(Value, String)> = (min..=max).map(|px| (json!(px), format!("{px} px"))).collect();
    let placeholder = font_placeholder(&value(s, "fontFamilySans"), spec["placeholder"].as_str().unwrap_or(""));
    let control = row()
        .gap(8.0)
        .center_y()
        .child(input(s, t, key, 176.0, &placeholder, false))
        .child(select(s, t, size, &format!("{title} size"), &options, select_width(size)));
    let hint = matches!(key, "fontFamilySans" | "fontFamilyCode").then(|| super::fonts::pending(value(s, key).as_str().unwrap_or(""))).flatten();
    match hint {
        Some(hint) => col().gap(4.0).items_end().child(control).child(text(hint).font(theme::TEXT_XS).color(t.c.muted_foreground)),
        None => control,
    }
}

/// The default family a font field falls back to: the platform's sans or monospace face, or
/// for the prompt, whatever the interface uses.
fn font_placeholder(interface: &Value, slot: &str) -> String {
    let sans = if cfg!(target_os = "macos") { "SF Pro" } else { "Inter" };
    match slot {
        "{interface}" => interface.as_str().map(str::trim).filter(|f| !f.is_empty()).unwrap_or(sans).to_owned(),
        "{sans}" => sans.to_owned(),
        "{mono}" => (if cfg!(target_os = "macos") { "SF Mono" } else { "System monospace" }).to_owned(),
        other => other.to_owned(),
    }
}

/// A row's `when` / `unless` condition, `[key, value]`: shown only while the setting equals
/// (or doesn't equal) the value.
pub fn visible(s: &App, spec: &Value) -> bool {
    let holds = |condition: &Value| {
        condition
            .as_array()
            .filter(|c| c.len() == 2)
            .map(|c| value(s, c[0].as_str().unwrap_or("")) == c[1])
    };
    holds(&spec["when"]).unwrap_or(true) && !holds(&spec["unless"]).unwrap_or(false)
}

/// Web `Textarea` under a row: multi-line, committed when it loses focus (the settings poll's
/// `commit_edits`), never on Enter.
pub fn textarea(s: &App, t: &T, key: &str, label: &str, placeholder: &str) -> El {
    let current = value(s, key);
    let edit = s.ui.settings.edits.get(key).cloned().unwrap_or_else(|| rune::TextEdit::new(super::input_text(&current)));
    let owned = key.to_owned();
    let field = rune::text_input(&edit, theme::TEXT_SM)
        .key(format!("setting-{key}"))
        .label(label)
        .placeholder(placeholder, t.c.muted_foreground.alpha_mul(0.72))
        .w_full()
        .min_h(70.0)
        .max_h(256.0)
        .px(11.0)
        .py(5.0)
        .radius(8.0)
        .border(1.0, t.c.input)
        .fill(t.c.input.alpha_mul(if t.dark { 0.32 } else { 0.0 }))
        .color(t.c.foreground)
        .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
        .focus_ring(3.0, -3.0, t.c.ring.alpha_mul(0.24))
        .on_edit(move |s: &mut App| {
            s.ui.settings
                .edits
                .entry(owned.clone())
                .or_insert_with(|| edit.clone())
        });
    col().w_full().max_w(672.0).pt(12.0).pb(10.0).child(field)
}

/// A settings text field: edits stay local until Enter or blur commit them through `update`.
pub fn input(s: &App, t: &T, key: &str, width: f32, placeholder: &str, default_size: bool) -> El {
    let current = value(s, key);
    let edit = s.ui.settings.edits.get(key).cloned().unwrap_or_else(|| rune::TextEdit::new(super::input_text(&current)));
    let (k1, k2) = (key.to_owned(), key.to_owned());
    let (height, pad) = if default_size { (32.0, 11.0) } else { (28.0, 9.0) };
    rune::text_input(&edit, theme::TEXT_SM)
        .key(format!("setting-{key}"))
        .placeholder(placeholder, t.c.muted_foreground.alpha_mul(0.72))
        .w(width)
        .h(height)
        .px(pad)
        .py((height - 20.0) / 2.0)
        .radius(8.0)
        .border(1.0, t.c.input)
        .fill(t.c.input.alpha_mul(if t.dark { 0.32 } else { 0.0 }))
        .color(t.c.foreground)
        .caret(t.c.foreground, t.c.primary.alpha_mul(0.3))
        .focus_ring(3.0, -3.0, t.c.ring.alpha_mul(0.24))
        .single_line()
        .on_edit(move |s: &mut App| {
            s.ui.settings
                .edits
                .entry(k1.clone())
                .or_insert_with(|| edit.clone())
        })
        .on_submit(move |s: &mut App, _| {
            if let Some(text) = s.ui.settings.edits.get(&k2).map(|e| e.text.clone()) {
                super::commit_text(s, &k2, &text);
            }
        })
}

pub fn section(title: &str, children: Vec<El>, t: &T) -> El {
    section_with_actions(title, vec![], children, t)
}

/// Web `SettingsSection` `headerAction`: buttons trail the title, `gap-2` apart.
/// The `SettingsSection` card alone (web `hideTitle`).
pub fn card(children: Vec<El>, t: &T) -> El {
    let mut group = col()
        .w_full()
        .padding(1.0)
        .radius(12.0)
        .border(1.0, t.c.border.alpha_mul(0.6))
        .fill(t.c.card.alpha_mul(0.4));
    for (i, child) in children.into_iter().enumerate() {
        if i > 0 {
            group = group.child(el().h(1.0).w_full().fill(t.c.border.alpha_mul(0.5)));
        }
        group = group.child(child);
    }
    group
}

pub fn section_with_actions(title: &str, actions: Vec<El>, children: Vec<El>, t: &T) -> El {
    col()
        .w_full()
        .gap(10.0)
        .child(
            row()
                .w_full()
                .h(28.0)
                .px(16.0)
                .gap(8.0)
                .center_y()
                .child(
                    text(title.to_owned())
                        .key(super::title_key(title))
                        .font(theme::weight(theme::TEXT_BASE, theme::SEMIBOLD))
                        .color(t.c.foreground)
                        .grow(1.0),
                )
                .children(actions),
        )
        .child(card(children, t))
}

fn slider(s: &App, t: &T, key: &str, title: &str) -> El {
    let n = value(s, key).as_f64().unwrap_or(100.0) as f32;
    let (min, max, step) = match key {
        "appearanceContrast" => (50.0, 200.0, 5.0),
        "glassOpacity" => (40.0, 100.0, 5.0),
        _ => (0.0, 400.0, 25.0),
    };
    let key = key.to_owned();
    row()
        .w(208.0)
        .center_y()
        .gap(12.0)
        .child(
            el().min_w(48.0).h(24.0).px(8.0).center().radius(6.0).fill(t.c.muted).child(text(format!(
                "{n}{}",
                if key == "panelAnimationDurationMs" {
                    " ms"
                } else {
                    "%"
                }
            ))
            .font(theme::weight(theme::MONO_XS, theme::MEDIUM))
            .color(t.c.foreground)),
        )
        .child(
            crate::kit::Slider::new(n)
                .range(min, max)
                .step(step)
                .width(148.0)
                .label(title)
                .on_change(move |s: &mut App, n, _| update(s, &key, json!(n)))
                .build(t),
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grouping_back_on_restores_the_path_mode() {
        assert_eq!(grouping_from_toggle(false, "repository_path"), "separate");
        assert_eq!(grouping_from_toggle(true, "repository_path"), "repository_path");
        assert_eq!(grouping_from_toggle(true, "separate"), "repository");
        assert_eq!(grouping_from_toggle(true, ""), "repository");
    }

    #[test]
    fn follow_up_copy_names_the_opposite_shortcut_for_each_send_key() {
        let tail = |shortcut| follow_up_description(shortcut, "Ctrl").split_once("run. ").unwrap().1.to_owned();
        assert_eq!(tail(None), "Press Ctrl + Enter to do the opposite for one message.");
        assert_eq!(tail(Some("enter")), "Press Ctrl + Enter to do the opposite for one message.");
        assert_eq!(tail(Some("mod-enter")), "Press Ctrl + Shift + Enter to do the opposite for one message.");
        assert_eq!(
            tail(Some("mod-enter-multiline")),
            "Press Ctrl + Enter for single-line prompts or Ctrl + Shift + Enter for multiline prompts to do the opposite for one message."
        );
    }
}
