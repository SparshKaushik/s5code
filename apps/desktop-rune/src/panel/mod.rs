//! Placeholder: owned by the right-panel surface worker. `browser` is the CDP-driven preview.

pub mod browser;

use rune::{Cx, El, el};

use crate::App;
use crate::theme::T;

#[derive(Default)]
pub struct Ui {}

fn scene(app: &mut App) {
    (crate::shell::SCENES[1].1)(app);
    app.ui.shell.panel_open = true;
}
pub const SCENES: &[(&str, fn(&mut App))] = &[("panel-picker", scene), ("panel-diff-empty", scene), ("panel-diff", scene), ("panel-files", scene), ("panel-terminal", scene), ("panel-browser", scene)];

pub fn view(_s: &App, _cx: &mut Cx<App>, t: &T) -> El {
    el().w(400.0).h_full().fill(t.c.background)
}

/// Timeline navigation hook until the diff panel is ported.
pub fn open_diff(app: &mut App, checkpoint: serde_json::Value) {
    app.ui.chat.timeline.diff_target = Some(checkpoint);
    app.ui.shell.panel_open = true;
}
