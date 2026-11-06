use crate::{
    App, icons, kit,
    theme::{self, AlphaMul, T},
};
use rune::{El, col, el, row, text};
use rune_kit::diff::{DiffMode, DiffState, DiffView};
use serde_json::json;

pub struct Ui {
    pub state: DiffState,
    pub from: u64,
    pub to: u64,
    pending: Option<u64>,
    error: Option<String>,
}
impl Default for Ui {
    fn default() -> Self {
        Self {
            state: DiffState::new(vec![]),
            from: 0,
            to: 0,
            pending: None,
            error: None,
        }
    }
}
pub fn load(app: &mut App) {
    let Some((id, _)) = super::context(app) else {
        return;
    };
    let count = app
        .state()
        .thread_views
        .get(&id)
        .and_then(|v| v.projection["checkpoints"].as_array())
        .map(|rows| {
            rows.iter()
                .filter_map(|r| r["appRunOrdinal"].as_u64())
                .max()
                .unwrap_or(0)
        })
        .unwrap_or(0);
    if app.ui.panel.diff.to == 0 {
        app.ui.panel.diff.to = count;
    }
    let ui = &mut app.ui.panel.diff;
    ui.error = None;
    ui.state
        .set_mode(if app.client_settings.values["diffLayout"] == "split" {
            DiffMode::Split
        } else {
            DiffMode::Unified
        });
    let payload = json!({"threadId":id,"fromTurnCount":ui.from,"toTurnCount":ui.to,"ignoreWhitespace":app.client_settings.values["diffIgnoreWhitespace"].as_bool().unwrap_or(true)});
    let (method, payload) = if ui.from == 0 {
        (
            "orchestration.getFullThreadDiff",
            json!({"threadId":id,"toTurnCount":ui.to,"ignoreWhitespace":app.client_settings.values["diffIgnoreWhitespace"].as_bool().unwrap_or(true)}),
        )
    } else {
        ("orchestration.getTurnDiff", payload)
    };
    let request = app.src.net().map(|net| net.request(method, payload));
    app.ui.panel.diff.pending = request;
}
pub fn poll(app: &mut App) -> bool {
    let Some(id) = app.ui.panel.diff.pending else {
        return false;
    };
    let Some(reply) = app.src.net().and_then(|n| n.take_reply(id)) else {
        return false;
    };
    app.ui.panel.diff.pending = None;
    match reply {
        Ok(v) => app
            .ui
            .panel
            .diff
            .state
            .set_files(rune_kit::diff::parse(v["diff"].as_str().unwrap_or(""))),
        Err(e) => app.ui.panel.diff.error = Some(e.to_string()),
    };
    true
}
pub fn view(app: &App, t: &T) -> El {
    let ui = &app.ui.panel.diff;
    let mut toolbar = row()
        .h(40.)
        .px(8.)
        .gap(4.)
        .center_y()
        .child(
            kit::Button::new(if ui.from == 0 {
                "All turns".into()
            } else {
                format!("Turn {}", ui.to)
            })
            .variant("ghost-muted")
            .size("xs")
            .on_press(|s: &mut App| {
                s.ui.panel.diff.from = 0;
                load(s);
            })
            .build(t),
        )
        .child(
            kit::Button::new("Previous turn")
                .icon(&icons::CHEVRON_LEFT)
                .variant("ghost-muted")
                .size("icon-xs")
                .disabled(ui.to == 0)
                .on_press(|s: &mut App| {
                    s.ui.panel.diff.to = s.ui.panel.diff.to.saturating_sub(1);
                    s.ui.panel.diff.from = s.ui.panel.diff.to.saturating_sub(1);
                    load(s);
                })
                .build(t),
        )
        .child(
            kit::Button::new("Next turn")
                .icon(&icons::CHEVRON_RIGHT)
                .variant("ghost-muted")
                .size("icon-xs")
                .on_press(|s: &mut App| {
                    s.ui.panel.diff.to += 1;
                    s.ui.panel.diff.from = s.ui.panel.diff.to.saturating_sub(1);
                    load(s);
                })
                .build(t),
        )
        .child(el().grow(1.));
    for (title, mode) in [("Unified", DiffMode::Unified), ("Split", DiffMode::Split)] {
        toolbar = toolbar.child(
            kit::Button::new(title)
                .variant(if ui.state.mode() == mode {
                    "secondary"
                } else {
                    "ghost-muted"
                })
                .size("xs")
                .on_press(move |s: &mut App| {
                    s.ui.panel.diff.state.set_mode(mode);
                    s.client_settings.values.insert(
                        "diffLayout".into(),
                        json!(if mode == DiffMode::Split {
                            "split"
                        } else {
                            "stacked"
                        }),
                    );
                    s.client_settings.save();
                })
                .build(t),
        );
    }
    let content = if let Some(error) = &ui.error {
        super::notice(t, error)
    } else if ui.pending.is_some() {
        super::notice(t, "Loading diff…")
    } else if ui.state.files().is_empty() {
        super::notice(t, "No net changes in this selection.")
    } else {
        let mut list = col().w_full().gap(2.).px(8.).py(6.);
        for (i, file) in ui.state.files().iter().enumerate() {
            let path = file.path().to_string();
            list = list.child(
                row()
                    .h(26.)
                    .w_full()
                    .radius(4.)
                    .hover(t.c.accent)
                    .focusable(true)
                    .on_press_tx(move |s: &mut App, tx| s.ui.panel.diff.state.scroll_to_file(i, tx))
                    .child(
                        text(path)
                            .font(theme::TEXT_XS)
                            .color(t.c.contrast_foreground),
                    ),
            );
        }
        col()
            .grow(1.)
            .min_h(0.)
            .child(list.max_h(140.).scroll(rune::ScrollSpec {
                stick_bottom: false,
            }))
            .child(
                DiffView::new(
                    |s: &App| &s.ui.panel.diff.state,
                    |s: &mut App| &mut s.ui.panel.diff.state,
                )
                .key("panel-diff-view")
                .build(app, &kit::theme(t))
                .on_key(|s: &mut App, e, tx| s.ui.panel.diff.state.key(e, tx)),
            )
    };
    col()
        .grow(1.)
        .min_h(0.)
        .child(toolbar)
        .child(el().h(1.).w_full().fill(t.c.border.alpha_mul(0.7)))
        .child(content)
}
pub fn fixture(app: &mut App) {
    let value: serde_json::Value =
        serde_json::from_str(include_str!("../../fixtures/panel/diff.json")).unwrap();
    app.ui
        .panel
        .diff
        .state
        .set_files(rune_kit::diff::parse(value["diff"].as_str().unwrap()));
}
