//! Placeholder: owned by the timeline worker.

use rune::{Cx, El, el};

use crate::App;
use crate::theme::T;

#[derive(Default)]
pub struct Ui {}

pub const SCENES: &[(&str, fn(&mut App))] = &[];

pub fn view(_s: &App, _cx: &mut Cx<App>, _t: &T, _thread_id: &str) -> El {
    el().grow(1.0)
}
