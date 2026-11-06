//! Placeholder: owned by the right-panel surface worker. `browser` is the CDP-driven preview.

pub mod browser;

use rune::{Cx, El, el};

use crate::App;
use crate::theme::T;

#[derive(Default)]
pub struct Ui {}

pub const SCENES: &[(&str, fn(&mut App))] = &[];

pub fn view(_s: &App, _cx: &mut Cx<App>, t: &T) -> El {
    el().w(400.0).h_full().fill(t.c.background)
}
