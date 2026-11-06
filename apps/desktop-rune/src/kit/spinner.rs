//! Visible loading work uses a renderer-owned loop; skeletons stay static.
use crate::icons;
use rune::{El, icon};
use rune_kit::Kit;
/// `Spinner::new().size("md").tone("muted").visible(in_view).build(&kit)`.
/// Feed viewport visibility to `visible` for loaders kept mounted outside a scroll viewport.
pub struct Spinner {
    size: f32,
    muted: bool,
    visible: bool,
}
impl Spinner {
    pub fn new() -> Self {
        Self {
            size: 16.,
            muted: false,
            visible: true,
        }
    }
    pub fn size(mut self, value: &str) -> Self {
        self.size = match value {
            "xs" => 12.,
            "sm" => 14.,
            "lg" => 20.,
            _ => 16.,
        };
        self
    }
    pub fn tone(mut self, value: &str) -> Self {
        self.muted = value == "muted";
        self
    }
    pub fn visible(mut self, value: bool) -> Self {
        self.visible = value;
        self
    }
    pub fn build(self, k: &Kit) -> El {
        let glyph = icon(&icons::LOADER_CIRCLE, self.size)
            .role("status")
            .label("Loading")
            .color(if self.muted {
                k.c.text_muted
            } else {
                k.c.text_base
            });
        if self.visible {
            glyph.looping(rune::LoopSpec::rotation(1.))
        } else {
            glyph.opacity(0.)
        }
    }
}
