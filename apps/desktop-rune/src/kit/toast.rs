//! Sonner-sized notifications. The owning surface drives deadlines from its poll hook.
use crate::theme::AlphaMul;
use rune::{El, Tx};
use rune_kit::Kit;

/// `Toast::new("saved", "Settings saved").description("Saved locally").build(&kit)`.
pub struct Toast(rune_kit::Toast);
impl Toast {
    pub fn new(key: impl Into<String>, title: impl Into<String>) -> Self {
        Self(rune_kit::Toast::new(key, title))
    }
    pub fn description(mut self, value: impl Into<String>) -> Self {
        self.0 = self.0.description(value);
        self
    }
    pub fn on_close<S: 'static>(mut self, f: impl Fn(&mut S) + 'static) -> Self {
        self.0 = self.0.on_close(f);
        self
    }
    pub fn build(self, k: &Kit) -> El {
        super::motion::popup(self.0.build(k))
            .w(356.)
            .padding(16.)
            .radius(8.)
            .fill(k.c.page)
            .border(1., k.c.border_base)
    }
}
/// Expanded stack: `ToastRegion::new().toast(toast.build(&kit)).build(&kit)`.
pub struct ToastRegion {
    toasts: Vec<El>,
    top: bool,
}
impl ToastRegion {
    pub fn new() -> Self {
        Self {
            toasts: vec![],
            top: true,
        }
    }
    pub fn toast(mut self, value: El) -> Self {
        self.toasts.push(value);
        self
    }
    pub fn position(mut self, position: &str) -> Self {
        self.top = position.starts_with("top");
        self
    }
    pub fn build(self, _k: &Kit) -> El {
        rune::col()
            .key("toast-region")
            .role("region")
            .label("Notifications")
            .transition(rune::Transition::none())
            .pin(
                None,
                if self.top { Some(24.) } else { None },
                Some(24.),
                if self.top { None } else { Some(24.) },
            )
            .gap(14.)
            .children(self.toasts.into_iter().rev().take(3))
    }
}

/// Surface-owned expiry state: `deadlines.push("saved", now_ms, &mut tx)`;
/// remove dismissed toast models when `deadlines.expire(now_ms)` returns their keys.
#[derive(Default)]
pub struct ToastDeadlines(Vec<(String, i64)>);
impl ToastDeadlines {
    pub fn push(&mut self, key: impl Into<String>, now_ms: i64, tx: &mut Tx) {
        let key = key.into();
        self.0.retain(|(existing, _)| *existing != key);
        self.0.push((key, now_ms + 4000));
        tx.wake_after(std::time::Duration::from_secs(4));
    }
    pub fn expire(&mut self, now_ms: i64) -> Vec<String> {
        let expired = self
            .0
            .iter()
            .filter(|(_, due)| *due <= now_ms)
            .map(|(key, _)| key.clone())
            .collect();
        self.0.retain(|(_, due)| *due > now_ms);
        expired
    }
}
