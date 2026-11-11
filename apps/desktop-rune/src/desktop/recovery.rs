//! Electron's DesktopStartupRecovery: while the bundled server keeps failing before it ever
//! started, say why, show what it wrote, and offer Copy details and Quit. Electron's offers to
//! update or to reinstall the last build that started need a release feed; Rune builds have none.

use rune::{El, ScrollSpec, Tx, col, el, row, text};

use crate::host::StartupFailure;
use crate::theme::{self, AlphaMul, T, layout};
use crate::{App, kit};

fn version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// What "Copy details" puts on the clipboard, as Electron's `copy` action.
fn details(f: &StartupFailure) -> String {
    format!("S5 Code {} ({} {}) could not start.\n{}\n\n{}", version(), std::env::consts::OS, std::env::consts::ARCH, f.reason, f.output)
}

pub fn view(f: &StartupFailure, t: &T) -> El {
    let output = f.output.trim();
    let copy = details(f);
    let logs = f.log.parent().map_or_else(|| f.log.display().to_string(), |d| d.display().to_string());
    let body = col()
        .w_full()
        .max_w(640.0)
        .grow(1.0)
        .min_h(0.0)
        .px(28.0)
        .py(28.0)
        .gap(14.0)
        .child(text("S5 Code could not start").font(theme::weight(theme::TEXT_LG, theme::SEMIBOLD)).color(t.c.foreground))
        .child(text(f.reason.clone()).font(theme::TEXT_SM).color(t.c.foreground).wrap())
        .child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Copy details").variant("outline").size("sm").build(t).on_press_tx(move |_: &mut App, tx: &mut Tx| tx.copy(copy.clone())))
                .child(kit::Button::new("Quit").variant("outline").size("sm").build(t).on_press_tx(crate::shell::quit::menu_quit)),
        )
        .child(
            kit::scroll_area(
                "startup-recovery-output",
                el().w_full()
                    .scroll(ScrollSpec { stick_bottom: true })
                    .child(
                        text(if output.is_empty() { "The server wrote no output.".to_owned() } else { output.to_owned() })
                            .font(theme::mono(theme::TEXT_XS))
                            .color(t.c.foreground)
                            .wrap()
                            .padding(10.0),
                    ),
                t,
            )
            .w_full()
            .grow(1.0)
            .min_h(60.0)
            .radius(6.0)
            .border(1.0, t.c.border)
            .fill(t.c.muted.alpha_mul(0.4)),
        )
        .child(
            text(format!("Version {}. S5 Code tries again in the background. Logs: {logs}", version()))
                .font(theme::TEXT_XS)
                .color(t.c.muted_foreground)
                .wrap(),
        );
    col()
        .key("startup-recovery")
        .role("main")
        .w_full()
        .h_full()
        .fill(t.c.background)
        .child(el().w_full().h(layout::TOPBAR_H).window_drag())
        .child(row().w_full().grow(1.0).min_h(0.0).justify_center().child(body))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copied_details_name_the_build_the_reason_and_the_output() {
        let f = StartupFailure { reason: "The server could not start (code=1).".into(), output: "Error: boom\n".into(), log: "/h/logs/server.log".into() };
        let d = details(&f);
        assert!(d.starts_with(&format!("S5 Code {} (", version())), "{d}");
        assert!(d.ends_with(" could not start.\nThe server could not start (code=1).\n\nError: boom\n"), "{d}");
    }
}
