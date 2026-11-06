//! Placeholder: owned by the composer/topbar/home worker.

use rune::{Cx, El, el};

use crate::App;
use crate::theme::T;

#[derive(Default)]
pub struct Ui {}

pub fn view(_s: &App, _cx: &mut Cx<App>, _t: &T) -> El {
    el()
}
