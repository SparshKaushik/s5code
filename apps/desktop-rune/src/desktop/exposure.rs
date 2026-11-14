//! Settings › Access › Network access and Tailscale HTTPS, as Electron's DesktopServerExposure:
//! the bundled server binds every interface instead of loopback, so paired devices on the network
//! reach it, and with Tailscale Serve it also answers on the machine's MagicDNS HTTPS name (the
//! server runs `tailscale serve` itself). The modes live in Electron's own `desktop-settings.json`
//! (defaults omitted), so both apps share them.

use std::cell::Cell;
use std::net::{IpAddr, Ipv4Addr, UdpSocket};
use std::process::{Command, Stdio};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

use rune::{El, TextEdit, col, el, row, text};
use serde_json::Value;

use crate::theme::{self, AlphaMul, T};
use crate::{App, kit};

const KEY: &str = "serverExposureMode";
const NETWORK: &str = "network-accessible";
const SERVE_KEY: &str = "tailscaleServeEnabled";
const SERVE_PORT_KEY: &str = "tailscaleServePort";
const DEFAULT_SERVE_PORT: u16 = 443;
/// Web keeps the chosen pairing endpoint in its UI state store; here it is a window pref.
const DEFAULT_ENDPOINT_KEY: &str = "defaultAdvertisedEndpointKey";
/// Electron's comma-separated endpoints offered beside the machine's own.
const CUSTOM_ENDPOINTS_ENV: &str = "T3CODE_DESKTOP_HTTPS_ENDPOINTS";
/// Web's `DESKTOP_NETWORK_ACCESS_STALE_TIME_MS`: revisiting Access re-reads after this.
const STALE_MS: i64 = 30_000;
const STATUS_TIMEOUT: Duration = Duration::from_millis(3_500);
const PROBE_TIMEOUT: Duration = Duration::from_millis(2_500);
/// After enabling Serve the restarted server needs a moment before its HTTPS name answers.
const SERVE_SETTLE: Duration = Duration::from_secs(20);

#[derive(Default)]
pub struct State {
    network: bool,
    /// Where other devices reach this machine, while shared.
    host: Option<Ipv4Addr>,
    serve: bool,
    serve_port: u16,
    /// The mode the confirm dialog asks to switch to.
    pending: Option<bool>,
    serve_dialog: Option<ServeDialog>,
    serve_port_edit: TextEdit,
    error: Option<String>,
    tailscale: Option<Tailscale>,
    resolving: Option<Receiver<Option<Tailscale>>>,
    resolved_at: i64,
    settle_until: Option<Instant>,
    /// Set while Access is on screen, so a stale snapshot refreshes as web's `revalidateOnMount`.
    viewed: Cell<bool>,
    expanded: bool,
    default_key: Option<String>,
    /// A headless scene shows the rows without a bundled server.
    scene: bool,
}

#[derive(Clone, Copy, PartialEq)]
enum ServeDialog {
    Enable,
    Disable,
}

/// What `tailscale status --json` says about this machine.
#[derive(Clone, Debug, Default, PartialEq)]
struct Tailscale {
    dns_name: Option<String>,
    ips: Vec<Ipv4Addr>,
    /// Serve answers on the MagicDNS name.
    serving: bool,
}

/// One URL this machine's server is reachable at, as web's `AdvertisedEndpoint` row needs it.
#[derive(Clone, Debug, PartialEq)]
pub struct Endpoint {
    /// Web's `endpointDefaultPreferenceKey`.
    pub key: String,
    pub label: String,
    pub url: String,
    pub available: bool,
    /// The core LAN endpoint, preferred until the user picks another.
    preferred: bool,
    loopback: bool,
    tailscale_https: bool,
    /// The hosted HTTPS app can pair through it.
    pub hosted_https: bool,
}

impl State {
    pub(super) fn load(&mut self) {
        let settings = settings();
        self.network = settings.get::<String>(KEY).as_deref() == Some(NETWORK);
        self.serve = settings.get::<bool>(SERVE_KEY) == Some(true);
        self.serve_port = serve_port(settings.get::<u64>(SERVE_PORT_KEY));
        self.host = self.network.then(lan_address).flatten();
        self.default_key = super::prefs().get(DEFAULT_ENDPOINT_KEY);
    }
}

fn settings() -> rune::prefs::Prefs {
    if rune::is_headless() {
        return rune::prefs::Prefs::memory();
    }
    rune::prefs::Prefs::at(crate::host::t3_home().join("userdata").join("desktop-settings.json"))
}

/// Electron's `normalizeTailscaleServePort`.
fn serve_port(value: Option<u64>) -> u16 {
    value.filter(|p| (1..=65_535).contains(p)).map_or(DEFAULT_SERVE_PORT, |p| p as u16)
}

/// What the bundled server binds on its next start.
pub fn bind_host() -> &'static str {
    if settings().get::<String>(KEY).as_deref() == Some(NETWORK) { "0.0.0.0" } else { "127.0.0.1" }
}

/// The Serve settings the bundled server starts with.
pub fn tailscale_serve() -> (bool, u16) {
    let settings = settings();
    (settings.get::<bool>(SERVE_KEY) == Some(true), serve_port(settings.get::<u64>(SERVE_PORT_KEY)))
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

fn is_tailscale_ip(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    a == 100 && (64..128).contains(&b)
}

fn usable_lan(ip: Ipv4Addr) -> bool {
    !ip.is_loopback() && !ip.is_link_local() && !ip.is_unspecified() && !is_tailscale_ip(ip)
}

fn port(s: &App) -> Option<u16> {
    s.ui.desktop.server.as_ref()?.0.trim_end_matches('/').rsplit(':').next()?.parse().ok()
}

fn custom_endpoints() -> Vec<String> {
    std::env::var(CUSTOM_ENDPOINTS_ENV).unwrap_or_default().split(',').map(str::trim).filter(|e| !e.is_empty()).map(str::to_owned).collect()
}

/// `parseTailscaleStatus`: the MagicDNS name without its trailing dot and the tailnet IPv4s.
fn parse_status(json: &str) -> Option<Tailscale> {
    let status: Value = serde_json::from_str(json).ok()?;
    let me = &status["Self"];
    let dns_name = me["DNSName"].as_str().map(|n| n.trim().trim_end_matches('.').to_owned()).filter(|n| !n.is_empty());
    let ips = me["TailscaleIPs"].as_array().into_iter().flatten().filter_map(|ip| ip.as_str()?.parse::<Ipv4Addr>().ok()).filter(|ip| is_tailscale_ip(*ip)).collect();
    Some(Tailscale { dns_name, ips, serving: false })
}

/// `buildTailscaleHttpsBaseUrl`: the port only when it isn't HTTPS's own.
fn serve_url(dns_name: &str, port: u16) -> String {
    if port == DEFAULT_SERVE_PORT { format!("https://{dns_name}/") } else { format!("https://{dns_name}:{port}/") }
}

/// `tailscale status --json` through the login shell's PATH, as Electron finds the CLI.
fn read_status() -> Option<Tailscale> {
    let mut command = Command::new("tailscale");
    #[cfg(unix)]
    command.env("PATH", super::shell_path::current());
    let mut child = command
        .args(["status", "--json"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let mut stdout = child.stdout.take()?;
    let reader = std::thread::spawn(move || {
        let mut out = String::new();
        std::io::Read::read_to_string(&mut stdout, &mut out).map(|_| out)
    });
    let started = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if started.elapsed() < STATUS_TIMEOUT => std::thread::sleep(Duration::from_millis(25)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    };
    let out = reader.join().ok()?.ok()?;
    if !status.success() {
        return None;
    }
    parse_status(&out)
}

/// `probeTailscaleHttpsEndpoint`: the environment descriptor answers 2xx.
fn probe(base: &str) -> bool {
    let agent = ureq::AgentBuilder::new().timeout(PROBE_TIMEOUT).build();
    let url = format!("{}/.well-known/t3/environment", base.trim_end_matches('/'));
    agent.get(&url).call().is_ok_and(|r| (200..300).contains(&r.status()))
}

fn resolve(serve: bool, serve_port: u16, settle_until: Option<Instant>) -> Option<Tailscale> {
    let mut status = read_status()?;
    if let (true, Some(name)) = (serve, status.dns_name.clone()) {
        let url = serve_url(&name, serve_port);
        loop {
            status.serving = probe(&url);
            if status.serving || settle_until.is_none_or(|until| Instant::now() >= until) {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    Some(status)
}

/// Re-reads Tailscale when the snapshot is missing or stale and Access is on screen. Electron
/// skips Tailscale until network access or Serve is on: the Mac App Store CLI can prompt.
pub(super) fn poll(s: &mut App) -> bool {
    let waker = s.ui.panel.waker.clone();
    let ui = &mut s.ui.desktop.exposure;
    let mut changed = false;
    if let Some(rx) = &ui.resolving
        && let Ok(tailscale) = rx.try_recv()
    {
        ui.resolving = None;
        ui.tailscale = tailscale;
        ui.resolved_at = crate::data::now_ms();
        ui.host = ui.network.then(lan_address).flatten();
        changed = true;
    }
    let now = crate::data::now_ms();
    let viewed = ui.viewed.take();
    let stale = ui.resolved_at == 0 || (viewed && now - ui.resolved_at > STALE_MS);
    if (ui.network || ui.serve) && ui.resolving.is_none() && stale && !rune::is_headless() {
        let (tx, rx) = channel();
        let (serve, serve_port, settle) = (ui.serve, ui.serve_port, ui.settle_until.take());
        std::thread::spawn(move || {
            let _ = tx.send(resolve(serve, serve_port, settle));
            if let Some(wake) = waker {
                wake();
            }
        });
        ui.resolving = Some(rx);
    }
    changed
}

/// Electron's `resolveDesktopCoreAdvertisedEndpoints` and the Tailscale provider's, in its order.
fn endpoints(port: u16, network: bool, lan: Option<Ipv4Addr>, custom: &[String], tailscale: Option<&Tailscale>, serve: bool, serve_port: u16) -> Vec<Endpoint> {
    let endpoint = |key: String, label: &str, url: String| Endpoint {
        key,
        label: label.into(),
        url,
        available: true,
        preferred: false,
        loopback: false,
        tailscale_https: false,
        hosted_https: false,
    };
    let mut out = vec![Endpoint { loopback: true, ..endpoint("desktop-core:loopback:http".into(), "This machine", format!("http://127.0.0.1:{port}")) }];
    if let (true, Some(lan)) = (network, lan) {
        out.push(Endpoint { preferred: true, ..endpoint("desktop-core:lan:http".into(), "Local network", format!("http://{lan}:{port}")) });
    }
    for url in custom {
        let https = url.to_ascii_lowercase().starts_with("https://");
        let (label, scheme) = if https { ("Custom HTTPS", "https") } else { ("Custom endpoint", url.split_once("://").map_or("unknown", |(s, _)| s)) };
        out.push(Endpoint { hosted_https: https, ..endpoint(format!("manual:public:{scheme}:{label}"), label, url.clone()) });
    }
    if let Some(ts) = tailscale {
        for ip in &ts.ips {
            out.push(endpoint("tailscale:ip:http".into(), "Tailscale IP", format!("http://{ip}:{port}")));
        }
        if let Some(name) = &ts.dns_name {
            let serving = serve && ts.serving;
            out.push(Endpoint {
                available: serving,
                tailscale_https: true,
                hosted_https: serving,
                ..endpoint("tailscale:magicdns:https".into(), "Tailscale HTTPS", serve_url(name, serve_port))
            });
        }
    }
    out
}

/// Web's `selectPairingEndpoint`.
fn select<'a>(endpoints: &'a [Endpoint], key: Option<&str>) -> Option<&'a Endpoint> {
    let available = || endpoints.iter().filter(|e| e.available);
    key.and_then(|k| available().find(|e| e.key == k))
        .or_else(|| available().find(|e| e.preferred))
        .or_else(|| available().find(|e| !e.loopback))
        .or_else(|| available().find(|e| e.hosted_https))
}

struct View {
    /// What the network row lists while shared: every endpoint but Tailscale HTTPS.
    network: Vec<Endpoint>,
    tailscale_https: Option<Endpoint>,
}

fn view(s: &App) -> View {
    let ui = &s.ui.desktop.exposure;
    let all = endpoints(port(s).unwrap_or(0), ui.network, ui.host, &custom_endpoints(), ui.tailscale.as_ref(), ui.serve, ui.serve_port);
    let (https, rest): (Vec<Endpoint>, Vec<Endpoint>) = all.into_iter().partition(|e| e.tailscale_https);
    View { network: if ui.network { rest } else { vec![] }, tailscale_https: https.into_iter().next() }
}

/// Shares this machine on 192.168.1.20:3773 with no bundled server, for headless scenes.
pub fn scene(s: &mut App) {
    s.ui.desktop.server = Some(("http://127.0.0.1:3773".into(), String::new()));
    let ui = &mut s.ui.desktop.exposure;
    ui.scene = true;
    ui.network = true;
    ui.host = Some(Ipv4Addr::new(192, 168, 1, 20));
}

/// Where pairing links point: web's `defaultDesktopAdvertisedEndpoint`. None while this window
/// doesn't run the bundled server.
pub fn pairing_endpoint(s: &App) -> Option<Endpoint> {
    if !crate::host::managed() {
        return None;
    }
    let v = view(s);
    let key = s.ui.desktop.exposure.default_key.as_deref();
    select(&v.network, key).or_else(|| select(v.tailscale_https.as_slice(), key)).cloned()
}

fn describe(network: bool, host: Option<Ipv4Addr>, port: Option<u16>) -> String {
    match (network, host, port) {
        (false, ..) => "Limited to this machine.".into(),
        (true, Some(host), Some(port)) => format!("Reachable at http://{host}:{port}"),
        (true, Some(host), None) => format!("Exposed on all interfaces. Pairing links use {host}."),
        (true, None, _) => "Exposed on all interfaces.".into(),
    }
}

/// Web `missingTailscaleEndpointDescription`.
fn missing_tailscale(network: bool, serve: bool) -> &'static str {
    if !network && !serve { "Turn on Network access to detect Tailscale." } else { "Start Tailscale to set up HTTPS access through MagicDNS." }
}

fn copy_col(title: &str, t: &T) -> El {
    col().grow(1.0).min_w(0.0).gap(4.0).child(text(title.to_owned()).font(theme::SM_MEDIUM).color(t.c.foreground))
}

fn muted(value: impl Into<String>, t: &T) -> El {
    text(value.into()).font(theme::TEXT_XS).color(t.c.muted_foreground).wrap()
}

/// Web `SettingsRow`: `px-4 py-3`, `gap-8` between the copy and the control.
fn setting_row(key: &str, copy: El, control: Option<El>) -> El {
    let mut r = row().key(key.to_owned()).w_full().px(16.0).py(12.0).gap(32.0).center_y().child(copy);
    if let Some(control) = control {
        r = r.child(control);
    }
    r
}

/// The rows of web's local backend block: Network access, the endpoints it expands to, and
/// Tailscale HTTPS. Empty while this window doesn't run the bundled server.
pub fn rows(s: &App, t: &T) -> Vec<El> {
    let Some(network) = network_row(s, t) else { return vec![] };
    let v = view(s);
    let mut out = vec![network];
    if s.ui.desktop.exposure.expanded && v.network.len() > 1 {
        let default = pairing_endpoint(s).map(|e| e.key);
        out.extend(v.network.iter().map(|e| endpoint_row(e, default.as_deref() == Some(e.key.as_str()), t)));
    }
    out.push(tailscale_row(s, &v, t));
    out
}

/// Web `renderNetworkAccessRow`, while this window runs the bundled server. Another server's
/// exposure is set where it starts.
pub fn network_row(s: &App, t: &T) -> Option<El> {
    let ui = &s.ui.desktop.exposure;
    if !crate::host::managed() && !ui.scene {
        return None;
    }
    ui.viewed.set(true);
    let on = ui.network;
    let v = view(s);
    let hidden = v.network.len().saturating_sub(1);
    let default = select(&v.network, ui.default_key.as_deref());
    let mut copy = copy_col("Network access", t);
    copy = match default.filter(|_| on) {
        // Web `NetworkAccessDescription`: the default endpoint, and `+N` to list the rest.
        Some(endpoint) => {
            let mut line = row().min_w(0.0).gap(4.0).child(muted("Reachable at", t)).child(muted(endpoint.url.clone(), t).min_w(0.0));
            if hidden > 0 {
                let expanded = ui.expanded;
                line = line.child(
                    text(if expanded { "Hide".to_owned() } else { format!("+{hidden}") })
                        .font(theme::weight(theme::TEXT_XS, 500.0))
                        .color(t.c.muted_foreground)
                        .role("button")
                        .label(if expanded { "Hide endpoints" } else { "Show all endpoints" })
                        .cursor(rune::Cursor::Pointer)
                        .on_press(|s: &mut App| s.ui.desktop.exposure.expanded ^= true),
                );
            }
            copy.child(line)
        }
        None => copy.child(muted(describe(on, ui.host, port(s)), t)),
    };
    if let Some(error) = &ui.error {
        copy = copy.child(text(error.clone()).font(theme::TEXT_XS).color(t.c.destructive).wrap());
    }
    let switch = kit::Switch::new(on)
        .label("Enable network access")
        .hide_label()
        .disabled(ui.pending.is_some())
        .on_toggle(move |s: &mut App| s.ui.desktop.exposure.pending = Some(!on))
        .build(&kit::theme(t));
    Some(setting_row("network-access", copy, Some(switch)))
}

/// Web `AdvertisedEndpointListRow` for the network list, which never holds Tailscale HTTPS.
fn endpoint_row(e: &Endpoint, is_default: bool, t: &T) -> El {
    let chip = |label: &str, ink: rune::Color, edge: rune::Color| {
        el().h(24.0).px(6.0).radius(6.0).border(1.0, edge).center_y().child(text(label.to_owned()).font(theme::TEXT_3XS).color(ink))
    };
    let mut heading = row()
        .grow(1.0)
        .min_w(0.0)
        .min_h(28.0)
        .gap(12.0)
        .center_y()
        .child(text(e.label.clone()).font(theme::SM_MEDIUM).color(t.c.foreground))
        .child(text(e.url.clone()).font(theme::TEXT_XS).color(t.c.muted_foreground).min_w(0.0));
    if !e.available {
        heading = heading.child(chip("Setup required", t.c.muted_foreground, t.c.border.alpha_mul(0.7)));
    }
    let action = if is_default {
        chip("Default", t.c.primary, t.c.primary.alpha_mul(0.3)).fill(t.c.primary.alpha_mul(0.1))
    } else {
        let key = e.key.clone();
        kit::Button::new("Set as default")
            .variant("outline")
            .size("xs")
            .on_press(move |s: &mut App| {
                let _ = super::prefs().set(DEFAULT_ENDPOINT_KEY, &key);
                s.ui.desktop.exposure.default_key = Some(key.clone());
            })
            .build(t)
    };
    let mut r = row().key(format!("endpoint-{}", e.url)).w_full().px(16.0).py(10.0).gap(12.0).center_y().child(heading).child(action);
    if !e.available {
        r = r.opacity(0.7);
    }
    r
}

/// Web `renderTailscaleRow`.
fn tailscale_row(s: &App, v: &View, t: &T) -> El {
    let ui = &s.ui.desktop.exposure;
    let mut copy = copy_col("Tailscale HTTPS", t);
    let control = match &v.tailscale_https {
        Some(e) => {
            copy = copy.child(muted(if e.available { e.url.clone() } else { "Use Tailscale Serve to expose this backend through a MagicDNS HTTPS URL.".into() }, t));
            let on = e.available;
            let busy = ui.serve_dialog.is_some() || ui.resolving.is_some() && ui.tailscale.is_none();
            Some(
                kit::Switch::new(on)
                    .label("Enable Tailscale HTTPS")
                    .hide_label()
                    .disabled(busy)
                    .on_toggle(move |s: &mut App| open_serve_dialog(s, !on))
                    .build(&kit::theme(t)),
            )
        }
        None => {
            copy = copy.child(muted(missing_tailscale(ui.network, ui.serve), t));
            None
        }
    };
    setting_row("tailscale-https", copy, control)
}

fn open_serve_dialog(s: &mut App, enable: bool) {
    let ui = &mut s.ui.desktop.exposure;
    ui.error = None;
    if enable {
        ui.serve_port_edit = TextEdit::new(ui.serve_port.max(1).to_string());
        ui.serve_dialog = Some(ServeDialog::Enable);
    } else {
        ui.serve_dialog = Some(ServeDialog::Disable);
    }
}

/// The HTTPS port field: digits only, 1 to 65535.
fn parse_port(text: &str) -> Option<u16> {
    let t = text.trim();
    (!t.is_empty() && t.bytes().all(|b| b.is_ascii_digit())).then(|| t.parse::<u16>().ok()).flatten().filter(|p| *p >= 1)
}

pub fn confirm(s: &App, t: &T, layers: &mut Vec<El>) {
    serve_confirm(s, t, layers);
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

/// Web's "Set up Tailscale HTTPS?" dialog and its "Disable Tailscale HTTPS?" alert.
fn serve_confirm(s: &App, t: &T, layers: &mut Vec<El>) {
    let ui = &s.ui.desktop.exposure;
    let close = |s: &mut App| s.ui.desktop.exposure.serve_dialog = None;
    match ui.serve_dialog {
        None => {}
        Some(ServeDialog::Disable) => kit::Dialog::new("tailscale-serve-disable", "Disable Tailscale HTTPS?")
            .open(true)
            .alert()
            .description("S5 Code will restart the local backend without Tailscale Serve.")
            .on_close(close)
            .child(
                row()
                    .gap(8.0)
                    .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                    .child(kit::Button::new("Restart and disable").variant("destructive").on_press(|s: &mut App| set_serve(s, false, None)).build(t)),
            )
            .build(t, layers),
        Some(ServeDialog::Enable) => {
            let port = parse_port(&ui.serve_port_edit.text);
            // Web `resolveTailscaleServeBaseUrl`: the MagicDNS URL at the typed port.
            let target = ui.tailscale.as_ref().and_then(|ts| ts.dns_name.as_deref()).map(|name| {
                let url = serve_url(name, port.unwrap_or(ui.serve_port));
                url.trim_end_matches('/').to_owned()
            });
            let mut field = col()
                .gap(8.0)
                .child(text("HTTPS port").font(theme::SM_MEDIUM).color(t.c.foreground))
                .child(
                    kit::Input::new(&ui.serve_port_edit)
                        .placeholder("443")
                        .invalid(port.is_none())
                        .on_edit(|s: &mut App| &mut s.ui.desktop.exposure.serve_port_edit)
                        .on_submit(|s: &mut App, _| {
                            if let Some(port) = parse_port(&s.ui.desktop.exposure.serve_port_edit.text) {
                                set_serve(s, true, Some(port));
                            }
                        })
                        .build(t)
                        .single_line(),
                );
            if port.is_none() {
                field = field.child(text("Enter a port from 1 to 65535.").font(theme::TEXT_XS).color(t.c.destructive));
            }
            let endpoint = col()
                .w_full()
                .px(12.0)
                .py(8.0)
                .radius(6.0)
                .border(1.0, t.c.border.alpha_mul(0.7))
                .fill(t.c.muted.alpha_mul(0.2))
                .child(text("HTTPS endpoint").font(theme::weight(theme::TEXT_XS, 500.0)).color(t.c.muted_foreground))
                .child(text(target.unwrap_or_else(|| "Pending MagicDNS endpoint".into())).font(theme::TEXT_SM).color(t.c.foreground).pt(4.0));
            kit::Dialog::new("tailscale-serve-setup", "Set up Tailscale HTTPS?")
                .open(true)
                .width(448.0)
                .description("S5 Code will restart the local backend with Tailscale Serve enabled and ask Tailscale to proxy HTTPS traffic to this backend.")
                .on_close(close)
                .child(col().w_full().gap(12.0).child(field).child(endpoint))
                .child(
                    row()
                        .gap(8.0)
                        .child(kit::Button::new("Cancel").variant("outline").on_press(close).build(t))
                        .child(
                            kit::Button::new("Enable")
                                .disabled(port.is_none())
                                .on_press(move |s: &mut App| {
                                    if let Some(port) = port {
                                        set_serve(s, true, Some(port));
                                    }
                                })
                                .build(t),
                        ),
                )
                .build(t, layers);
        }
    }
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
    ui.expanded = false;
    ui.resolved_at = 0;
    crate::host::restart();
}

/// Electron's `setTailscaleServeEnabled`: persists Serve and its port (defaults omitted), and
/// restarts the server only when either changed.
fn set_serve(s: &mut App, enabled: bool, port: Option<u16>) {
    let ui = &mut s.ui.desktop.exposure;
    ui.serve_dialog = None;
    ui.error = None;
    let port = port.unwrap_or(ui.serve_port);
    if ui.serve == enabled && ui.serve_port == port && (!enabled || ui.tailscale.as_ref().is_some_and(|ts| ts.serving)) {
        return;
    }
    let mut prefs = settings();
    let saved = if enabled { prefs.set(SERVE_KEY, &true) } else { prefs.remove(SERVE_KEY) }
        .and_then(|_| if port == DEFAULT_SERVE_PORT { prefs.remove(SERVE_PORT_KEY) } else { prefs.set(SERVE_PORT_KEY, &port) });
    if saved.is_err() {
        ui.error = Some(format!("Failed to persist desktop Tailscale Serve settings (enabled: {enabled}, port: {port})."));
        return;
    }
    ui.serve = enabled;
    ui.serve_port = port;
    if let Some(ts) = &mut ui.tailscale {
        ts.serving = false;
    }
    ui.resolved_at = 0;
    ui.settle_until = enabled.then(|| Instant::now() + SERVE_SETTLE);
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
        assert_eq!(missing_tailscale(false, false), "Turn on Network access to detect Tailscale.");
        assert_eq!(missing_tailscale(true, false), "Start Tailscale to set up HTTPS access through MagicDNS.");
        let mut s = App::new();
        s.ui.desktop.server = Some(("http://127.0.0.1:3774".into(), "t".into()));
        assert_eq!(port(&s), Some(3774));
    }

    #[test]
    fn reads_the_tailnet_from_tailscale_status() {
        let json = r#"{"Self":{"DNSName":"box.tail1234.ts.net.","TailscaleIPs":["100.101.5.6","fd7a:115c:a1e0::1","10.0.0.2"]}}"#;
        let ts = parse_status(json).unwrap();
        assert_eq!(ts.dns_name.as_deref(), Some("box.tail1234.ts.net"));
        assert_eq!(ts.ips, vec![Ipv4Addr::new(100, 101, 5, 6)]);
        assert_eq!(parse_status(r#"{"Self":{"DNSName":" "}}"#).unwrap(), Tailscale::default());
        assert_eq!(parse_status("not json"), None);
        assert_eq!(serve_url("box.ts.net", 443), "https://box.ts.net/");
        assert_eq!(serve_url("box.ts.net", 8443), "https://box.ts.net:8443/");
        assert_eq!(serve_port(Some(8443)), 8443);
        assert_eq!(serve_port(Some(0)), 443);
        assert_eq!(serve_port(Some(70_000)), 443);
        assert_eq!(serve_port(None), 443);
        assert_eq!(parse_port(" 8443 "), Some(8443));
        for bad in ["", "0", "65536", "-1", "44.3", "abc"] {
            assert_eq!(parse_port(bad), None, "{bad}");
        }
    }

    #[test]
    fn lists_endpoints_and_picks_the_pairing_default_as_web_does() {
        let lan = Some(Ipv4Addr::new(192, 168, 1, 20));
        let ts = Tailscale { dns_name: Some("box.ts.net".into()), ips: vec![Ipv4Addr::new(100, 101, 5, 6)], serving: true };
        let custom = vec!["https://t3.example.com".to_owned(), "http://box.lan:3773".to_owned()];
        let all = endpoints(3773, true, lan, &custom, Some(&ts), true, 443);
        let urls: Vec<(&str, &str)> = all.iter().map(|e| (e.label.as_str(), e.url.as_str())).collect();
        assert_eq!(
            urls,
            [
                ("This machine", "http://127.0.0.1:3773"),
                ("Local network", "http://192.168.1.20:3773"),
                ("Custom HTTPS", "https://t3.example.com"),
                ("Custom endpoint", "http://box.lan:3773"),
                ("Tailscale IP", "http://100.101.5.6:3773"),
                ("Tailscale HTTPS", "https://box.ts.net/"),
            ]
        );
        assert_eq!(all[2].key, "manual:public:https:Custom HTTPS");
        assert!(all[2].hosted_https && all[5].hosted_https && all[5].available);

        // The LAN endpoint is the default until the user picks another that is still listed.
        assert_eq!(select(&all, None).unwrap().label, "Local network");
        assert_eq!(select(&all, Some("tailscale:ip:http")).unwrap().label, "Tailscale IP");
        assert_eq!(select(&all, Some("gone")).unwrap().label, "Local network");

        // Serve off: the MagicDNS name is listed but unavailable, so never the default.
        let off = endpoints(3773, false, lan, &[], Some(&ts), false, 443);
        assert_eq!(off.iter().map(|e| e.label.as_str()).collect::<Vec<_>>(), ["This machine", "Tailscale IP", "Tailscale HTTPS"]);
        assert!(!off[2].available);
        assert_eq!(select(&off[2..], Some("tailscale:magicdns:https")), None);
        // Only loopback: no endpoint another device could use but the loopback fallback is skipped.
        let local = endpoints(3773, false, None, &[], None, false, 443);
        assert_eq!(select(&local, None), None);
    }
}
