//! Overlay wrappers keep Rune positioning and tooltip semantics with the web's surface.
use super::menu::glass;
use crate::theme;
use rune::{El, col, row, text};
use rune_kit::{Kit, Placement};

/// `Tooltip::new("save").trigger(button).label("Save").open(hovered).build(&kit, &mut layer)`.
/// Attach `hover_delays(Duration::from_millis(600), Duration::ZERO)` to a trigger's
/// `on_hover_change` to control open state without repainting during the delay.
pub struct Tooltip(rune_kit::Tooltip);
impl Tooltip {
    pub fn new(key: impl Into<String>) -> Self {
        Self(rune_kit::Tooltip::new(key).placement(Placement::Top))
    }
    pub fn trigger(mut self, value: El) -> Self {
        self.0 = self.0.trigger(value);
        self
    }
    /// A stateful trigger with Base UI's 600ms resting delay.
    pub fn hover_trigger<S: 'static>(
        mut self,
        trigger: El,
        f: impl Fn(&mut S, bool) + 'static,
    ) -> Self {
        self.0 = self.0.trigger(
            trigger
                .hover_delays(
                    std::time::Duration::from_millis(600),
                    std::time::Duration::ZERO,
                )
                .on_hover_change(move |s: &mut S, inside, _| f(s, inside)),
        );
        self
    }
    pub fn text(self, _k: &Kit, value: impl Into<String>) -> Self {
        self.label(value)
    }
    pub fn label(mut self, value: impl Into<String>) -> Self {
        self.0 = self.0.label(value);
        self
    }
    pub fn child(mut self, value: El) -> Self {
        self.0 = self.0.child(value);
        self
    }
    pub fn open(mut self, value: bool) -> Self {
        self.0 = self.0.open(value);
        self
    }
    pub fn placement(mut self, value: Placement) -> Self {
        self.0 = self.0.placement(value);
        self
    }
    pub fn build(self, k: &Kit, layer: &mut Vec<El>) -> El {
        let style = rune_kit::TooltipStyle::neutral(k.dark)
            .fill(k.c.layer_01)
            .border(1., k.c.border_base)
            .radius(6.)
            .padding(8., 4.)
            .max_width(320.)
            .font(theme::TEXT_XS.lh(16.5))
            .ink(k.c.text_base);
        self.0.render(&style, layer).self_start()
    }
}

/// `Popover::new("info").trigger(button).child(content).open(open).build(&kit, &mut layer)`.
pub struct Popover {
    key: String,
    trigger: Option<El>,
    title: Option<String>,
    label: Option<String>,
    description: Option<String>,
    body: Vec<El>,
    open: bool,
    placement: Placement,
    gutter: f32,
    padding: f32,
    width: Option<f32>,
    dismiss: Option<(El, Box<dyn FnOnce(El) -> El>)>,
}
impl Popover {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            trigger: None,
            title: None,
            label: None,
            description: None,
            body: vec![],
            open: false,
            placement: Placement::Bottom,
            gutter: 4.,
            padding: 16.,
            width: None,
            dismiss: None,
        }
    }
    pub fn trigger(mut self, value: El) -> Self {
        self.trigger = Some(value);
        self
    }
    pub fn label(mut self, value: impl Into<String>) -> Self {
        self.label = Some(value.into());
        self
    }
    pub fn title(mut self, value: impl Into<String>) -> Self {
        self.title = Some(value.into());
        self
    }
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.description = Some(value.into());
        self
    }
    pub fn child(mut self, value: El) -> Self {
        self.body.push(value);
        self
    }
    pub fn open(mut self, value: bool) -> Self {
        self.open = value;
        self
    }
    pub fn placement(mut self, value: Placement) -> Self {
        self.placement = value;
        self
    }
    pub fn gutter(mut self, value: f32) -> Self {
        self.gutter = value;
        self
    }
    pub fn padding(mut self, value: f32) -> Self {
        self.padding = value;
        self
    }
    pub fn width(mut self, value: &str) -> Self {
        self.width = match value {
            "sm" => Some(256.),
            "md" => Some(320.),
            "lg" => Some(384.),
            _ => None,
        };
        self
    }
    pub fn on_close<S: 'static>(mut self, f: impl Fn(&mut S) + Clone + 'static) -> Self {
        let outside = f.clone();
        self.dismiss = Some((
            rune_kit::scrim(format!("{}-scrim", self.key), outside),
            Box::new(move |e| e.modal(true, f)),
        ));
        self
    }
    pub fn build(self, k: &Kit, layer: &mut Vec<El>) -> El {
        let key = format!("{}-trigger", self.key);
        let trigger = row()
            .self_start()
            .key(key.clone())
            .children(self.trigger.map(|e| e.expanded(self.open)));
        if !self.open {
            return trigger;
        }
        let mut content = col()
            .key(format!("{}-content", self.key))
            .role("dialog")
            .padding(self.padding)
            .gap(8.)
            .min_h(0.)
            .focus_return(key.clone());
        if let Some(label) = self.label {
            content = content.label(label);
        }
        if let Some(width) = self.width {
            content = content.w(width);
        }
        if let Some(title) = self.title {
            content = content.label(title.clone()).child(
                text(title)
                    .font(theme::weight(theme::TEXT_SM, 600.).lh(14.))
                    .color(k.c.text_base),
            );
        }
        if let Some(description) = self.description {
            content = content
                .description(description.clone())
                .child(text(description).font(theme::TEXT_SM).color(k.c.text_muted));
        }
        content = glass(content.children(self.body), k, false);
        if let Some((scrim, modal)) = self.dismiss {
            layer.push(scrim);
            content = modal(content);
        }
        layer.push(rune_kit::popover::anchored(
            k,
            content,
            &key,
            self.placement,
            self.gutter,
        ));
        trigger
    }
}
