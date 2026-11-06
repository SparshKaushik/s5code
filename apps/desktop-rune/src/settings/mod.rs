//! Placeholder: owned by the settings surface worker.

use rune::{Cx, El, el};

use crate::App;
use crate::theme::T;

#[derive(Default)]
pub struct Ui {}

pub const SCENES: &[(&str, fn(&mut App))] = &[];

pub fn view(_s: &App, _cx: &mut Cx<App>, t: &T) -> El {
    el().grow(1.0).h_full().fill(t.c.background)
}
