//! Sidebar v2. Placeholder: owned by the sidebar surface worker.

use rune::{Cx, El, col, el, text};

use crate::App;
use crate::data::s;
use crate::theme::{self, T, layout};

#[derive(Default)]
pub struct Ui {}

pub const SCENES: &[(&str, fn(&mut App))] = &[];

pub fn view(st: &App, _cx: &mut Cx<App>, t: &T) -> El {
    let mut list = col().key("sidebar").w(layout::SIDEBAR_W).h_full().fill(t.sb.sidebar).pt(layout::TOPBAR_H).px(8.0).gap(4.0);
    for th in &st.state().threads {
        list = list.child(text(s(th, "title").to_string()).font(theme::SM_MEDIUM).color(t.sb.contrast_foreground));
    }
    list.child(el().absolute(layout::SIDEBAR_W - 1.0, 0.0).w(1.0).h_full().fill(t.sb.contrast_sidebar_border))
}
