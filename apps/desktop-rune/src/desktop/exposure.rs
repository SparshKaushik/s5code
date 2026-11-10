//! Settings › Access › Network access, as Electron's DesktopServerExposure: the bundled server
//! binds every interface instead of loopback, so paired devices on the network reach it. The
//! mode lives in Electron's own `desktop-settings.json` (default omitted), so both apps share it.

use std::net::{IpAddr, Ipv4Addr, UdpSocket};

use rune::{El, col, row, text};

use crate::theme::{self, T};
use crate::{App, kit};

const KEY: &str = "serverExposureMode";
const NETWORK: &str = "network-accessible";

#[derive(Default)]
pub struct State {
    network: bool,
    /// Where other devices reach this machine, while shared.
    host: Option<Ipv4Addr>,
    /// The mode the confirm dialog asks to switch to.
    pending: Option<bool>,
    error: Option<String>,
}

impl State {
    pub(super) fn load(&mut self) {
        self.network = network_accessible();
        self.host = self.network.then(lan_address).flatten();
    }
}

fn settings() -> rune::prefs::Prefs {
    if rune::is_headless() {
        return rune::prefs::Prefs::memory();
    }
    rune::prefs::Prefs::at(crate::host::t3_home().join("userdata").join("desktop-settings.json"))
}

fn network_accessible() -> bool {
    settings().get::<String>(KEY).as_deref() == Some(NETWORK)
}

/// What the bundled server binds on its next start.
pub fn bind_host() -> &'static str {
    if network_accessible() { "0.0.0.0" } else { "127.0.0.1" }
}

/// The default route's IPv4 (connecting a UDP socket sends nothing), as Electron's
/// `resolveLanAdvertisedHost`: never loopback, link-local or a Tailscale address.
fn lan_address() -> Option<Ipv4Addr> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match socket.local_addr().ok()?.ip() {
        IpAddr::V4(ip) if usable_lan(ip) => Some(ip),
        _ => None,
    }
}

fn usable_lan(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    let tailscale = a == 100 && (64..128).contains(&b);
    !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified() && !tailscale
}

fn port(s: &App) -> Option<u16> {
    s.ui.desktop.server.as_ref()?.0.trim_end_matches('/').rsplit(':').next()?.parse().ok()
}

fn describe(network: bool, host: Option<Ipv4Addr>, port: Option<u16>) -> String {
    match (network, host, port) {
        (false, ..) => "Limited to this machine.".into(),
        (true, Some(host), Some(port)) => format!("Reachable at http://{host}:{port}"),
        (true, Some(host), None) => format!("Exposed on all interfaces. Pairing links use {host}."),
        (true, None, _) => "Exposed on all interfaces.".into(),
    }
}

/// Web `renderNetworkAccessRow`, while this window runs the bundled server. Another server's
/// exposure is set where it starts.
pub fn network_row(s: &App, t: &T) -> Option<El> {
    if !crate::host::managed() {
        return None;
    }
    let ui = &s.ui.desktop.exposure;
    let on = ui.network;
    let mut copy = col()
        .grow(1.0)
        .min_w(0.0)
        .gap(4.0)
        .child(text("Network access").font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text(describe(on, ui.host, port(s))).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap());
    if let Some(error) = &ui.error {
        copy = copy.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
    }
    let switch = kit::Switch::new(on)
        .label("Enable network access")
        .hide_label()
        .disabled(ui.pending.is_some())
        .on_toggle(move |s: &mut App| s.ui.desktop.exposure.pending = Some(!on))
        .build(&kit::theme(t));
    Some(row().key("network-access").w_full().padding(16.0).gap(12.0).center_y().child(copy).child(switch))
}

pub fn confirm(s: &App, t: &T, layers: &mut Vec<El>) {
    let Some(network) = s.ui.desktop.exposure.pending else { return };
    let close = |s: &mut App| s.ui.desktop.exposure.pending = None;
    let (title, description, action) = if network {
        (
            "Enable network access?",
            "Let your other devices connect to S5 Code over the network. Pair devices to give them access. S5 Code's server will restart.",
            "Restart and enable",
        )
    } else {
        (
            "Disable network access?",
            "Devices connected over your local network will disconnect. Existing tunnels, such as T3 Connect or Tailscale HTTPS, keep working. S5 Code's server will restart.",
            "Restart and disable",
        )
    };
    kit::Dialog::new("network-access-confirm", title)
        .open(true)
        .alert()
        .description(description)
        .on_close(close)
        .child(
            row()
                .gap(8.0)
                .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                .child(kit::Button::new(action).on_press(move |s: &mut App| set(s, network)).build(t)),
        )
        .build(t, layers);
}

/// Electron's `setServerExposureMode`, then the server restart its relaunch did.
fn set(s: &mut App, network: bool) {
    let port = port(s);
    let ui = &mut s.ui.desktop.exposure;
    ui.pending = None;
    ui.error = None;
    let host = lan_address();
    if network && host.is_none() {
        let on = port.map_or_else(String::new, |p| format!(" on port {p}"));
        ui.error = Some(format!("No reachable network address is available for desktop network access{on}."));
        return;
    }
    let mut prefs = settings();
    let saved = if network { prefs.set(KEY, NETWORK) } else { prefs.remove(KEY) };
    if saved.is_err() {
        ui.error = Some("Failed to update network exposure.".into());
        return;
    }
    ui.network = network;
    ui.host = host;
    crate::host::restart();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advertises_a_lan_address_never_loopback_link_local_or_tailscale() {
        for (ip, usable) in [
            ([192, 168, 1, 20], true),
            ([10, 0, 0, 5], true),
            ([100, 63, 255, 1], true),
            ([127, 0, 0, 1], false),
            ([169, 254, 3, 4], false),
            ([100, 64, 0, 1], false),
            ([100, 127, 255, 254], false),
            ([0, 0, 0, 0], false),
        ] {
            assert_eq!(usable_lan(Ipv4Addr::from(ip)), usable, "{ip:?}");
        }
    }

    #[test]
    fn describes_the_exposure_as_electron_does() {
        let lan = Some(Ipv4Addr::new(192, 168, 1, 20));
        assert_eq!(describe(false, lan, Some(3773)), "Limited to this machine.");
        assert_eq!(describe(true, lan, Some(3773)), "Reachable at http://192.168.1.20:3773");
        assert_eq!(describe(true, lan, None), "Exposed on all interfaces. Pairing links use 192.168.1.20.");
        assert_eq!(describe(true, None, Some(3773)), "Exposed on all interfaces.");
        let mut s = App::new();
        s.ui.desktop.server = Some(("http://127.0.0.1:3774".into(), "t".into()));
        assert_eq!(port(&s), Some(3774));
    }
}
