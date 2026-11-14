use super::{controls as c, request};
use crate::{
    App, Route,
    theme::{self, T},
};
use rune::{El, col, row, text};
use serde_json::json;

pub fn view(s: &App, t: &T) -> El {
    let version = s
        .state()
        .config
        .as_ref()
        .and_then(|c| c["environment"]["serverVersion"].as_str())
        .unwrap_or("Unknown");
    let update = s.ui.settings.results.get("server.getNpmRelease");
    let target = update.and_then(|v| v["version"].as_str());
    let can_update = s
        .state()
        .config
        .as_ref()
        .and_then(|c| c["environment"]["capabilities"]["serverSelfUpdate"].as_str())
        .is_some_and(|capability| capability != "unsupported");
    // Web's About version row: Electron's app update, which Rune builds have no feed for.
    let mut app = vec![about_row(
        t,
        &format!("Version {}", env!("CARGO_PKG_VERSION")),
        crate::desktop::NO_UPDATE_FEED,
        c::button("Check for Updates", t).on_press(crate::desktop::check_for_updates),
    )];
    // Web keeps this under Connections (`ServerUpdateAction`); here it names the server apart.
    let mut controls = row().gap(8.0).child(
        c::button("Check Server Release", t)
            .on_press(|s: &mut App| request(s, "server.getNpmRelease", json!({"spec":"latest"}))),
    );
    if let Some(target) = target.filter(|target| *target != version) {
        let target = target.to_owned();
        controls = controls.child(
            c::button("Update", t)
                .disabled(!can_update)
                .on_press(move |s: &mut App| s.ui.settings.update_version = Some(target.clone())),
        );
    }
    app.push(about_row(
        t,
        "Server",
        &match target {
            Some(v) if v != version => format!("Version {version} · Latest release: {v}"),
            _ => format!("Version {version}"),
        },
        controls,
    ));
    // Web's `CliCommandSettingsRow`, desktop only.
    #[cfg(unix)]
    if let Some(cli) = crate::desktop::cli::row(s) {
        let button = c::button(cli.action, t).disabled(cli.disabled).on_press(crate::desktop::cli::press);
        app.push(about_row(t, "t3 command", &cli.description, button));
    }
    col()
        .w_full()
        .gap(32.0)
        .child(c::section("App", app, t))
        .child(c::section(
            "Legal",
            vec![
                about_row(
                    t,
                    "Privacy policy",
                    "How we handle your data, including the anonymous usage data T3 Code collects.",
                    c::button("View policy", t).on_press(|s: &mut App| {
                        super::auth::open_url(s, "https://t3.codes/privacy-policy")
                    }),
                ),
                about_row(
                    t,
                    "Open source licenses",
                    "Notices for dependencies, assets, and optional tools used by T3 Code.",
                    c::button("View licenses", t).on_press(|s: &mut App| {
                        s.navigate(Route::Settings("open-source-licenses".into()))
                    }),
                ),
            ],
            t,
        ))
}
/// A `SettingsRow`: copy on the left, control on the right, `px-4 py-3` like every settings row.
pub fn about_row(t: &T, title: &str, description: &str, control: El) -> El {
    row()
        .w_full()
        .px(16.0)
        .py(12.0)
        .gap(32.0)
        .center_y()
        .child(
            col()
                .grow(1.0)
                .min_w(0.0)
                .gap(4.0)
                .child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(text(description.to_owned()).font(theme::TEXT_XS.lh(18.0)).color(t.c.muted_foreground).wrap()),
        )
        .child(control)
}
pub fn update_dialog(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(version) = &s.ui.settings.update_version else {
        return;
    };
    let version = version.clone();
    crate::kit::Dialog::new("update-server", "Update server?")
        .open(true)
        .alert()
        .description(format!(
            "Install {version}? The server will restart and connected clients will reconnect."
        ))
        .on_close(|s: &mut App| s.ui.settings.update_version = None)
        .child(
            row()
                .gap(8.0)
                .child(
                    c::button("Cancel", t)
                        .on_press(|s: &mut App| s.ui.settings.update_version = None),
                )
                .child(
                    crate::kit::Button::new("Update server")
                        .on_press(move |s: &mut App| {
                            request(s, "server.updateServer", json!({"targetVersion":version}));
                            s.ui.settings.update_version = None;
                        })
                        .build(t),
                ),
        )
        .build(t, layers);
}
