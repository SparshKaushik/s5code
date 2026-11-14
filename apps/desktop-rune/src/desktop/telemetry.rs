//! Electron's DesktopTelemetryPublisher on the server's fd 4: the host power state that the
//! server's background policy and resource monitor read. Without it the server's power state
//! stays stale, so pausing on battery or a locked screen never happens.
//!
//! Rune advertises no control fd 5. The server would then offer remote clients desktop app
//! updates, and Rune has no updater to run them; without it the default intervals hold.

use std::io::Write;
use std::process::Command;
use std::time::Duration;

/// `DEFAULT_ACTIVE_INTERVAL_MS`: the server marks the channel stale after 90 s of silence.
const INTERVAL: Duration = Duration::from_secs(30);

/// The hello, then a snapshot every interval until the server closes its end.
pub fn publish(mut writer: std::io::PipeWriter) {
    std::thread::spawn(move || {
        let pid = std::process::id();
        if writeln!(writer, "{}", serde_json::json!({"version": 1, "type": "desktopTelemetryHello", "electronPid": pid})).is_err() {
            return;
        }
        for sequence in 0u64.. {
            let line = snapshot(sequence, pid, crate::data::now_ms(), &crate::shell::palette::add_project::iso_now(), sample());
            if writeln!(writer, "{line}").is_err() {
                return;
            }
            std::thread::sleep(INTERVAL);
        }
    });
}

/// What this machine says about its power: `"true"`, `"false"` or `"unknown"` each.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Power {
    pub on_battery: &'static str,
    pub locked: &'static str,
    pub thermal: &'static str,
}

fn flag(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "unknown",
    }
}

/// `DesktopHostTelemetrySnapshot`. Electron's own process metrics are only sent while the
/// server asks for diagnostics, which needs fd 5.
fn snapshot(sequence: u64, pid: u32, now_ms: i64, now_iso: &str, power: Power) -> serde_json::Value {
    serde_json::json!({
        "version": 1,
        "type": "desktopTelemetry",
        "sequence": sequence,
        "sampledAtUnixMs": now_ms.max(0),
        "electronPid": pid,
        "power": {
            "source": "electron-main",
            "idle": "unknown",
            "idleSeconds": null,
            "locked": power.locked,
            "onBattery": power.on_battery,
            "lowPowerMode": "unknown",
            "thermalState": power.thermal,
            "suspended": false,
            "stale": false,
            "updatedAt": now_iso,
        },
        "speedLimitPercent": null,
        "electronProcesses": [],
    })
}

#[cfg(target_os = "macos")]
fn sample() -> Power {
    let batt = Command::new("/usr/bin/pmset").args(["-g", "batt"]).output().ok().map(|o| String::from_utf8_lossy(&o.stdout).into_owned());
    Power { on_battery: flag(batt.as_deref().and_then(pmset_on_battery)), locked: "unknown", thermal: super::macos::thermal_state() }
}

#[cfg(not(target_os = "macos"))]
fn sample() -> Power {
    let supplies = std::fs::read_dir("/sys/class/power_supply")
        .map(|dir| {
            dir.flatten()
                .map(|e| {
                    let read = |name: &str| std::fs::read_to_string(e.path().join(name)).unwrap_or_default().trim().to_owned();
                    (read("type"), read("online"))
                })
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    let locked = std::env::var("XDG_SESSION_ID").ok().and_then(|id| {
        let out = Command::new("loginctl").args(["show-session", &id, "-p", "LockedHint", "--value"]).output().ok()?;
        out.status.success().then(|| String::from_utf8_lossy(&out.stdout).trim() == "yes")
    });
    Power { on_battery: flag(linux_on_battery(&supplies)), locked: flag(locked), thermal: "unknown" }
}

/// Electron's `isOnBatteryPower` from sysfs: on battery when a battery exists and no mains
/// supply is online; a machine without a battery never is.
#[cfg_attr(target_os = "macos", allow(dead_code))]
fn linux_on_battery(supplies: &[(String, String)]) -> Option<bool> {
    let battery = supplies.iter().any(|(kind, _)| kind == "Battery");
    let mains: Vec<&str> = supplies.iter().filter(|(kind, _)| kind == "Mains" || kind == "USB").map(|(_, online)| online.as_str()).collect();
    match (battery, mains.is_empty()) {
        (false, _) => Some(false),
        (true, true) => None,
        (true, false) => Some(!mains.contains(&"1")),
    }
}

/// `pmset -g batt`'s first line: "Now drawing from 'Battery Power'".
#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
fn pmset_on_battery(out: &str) -> Option<bool> {
    let line = out.lines().next()?;
    if line.contains("'Battery Power'") {
        Some(true)
    } else if line.contains("'AC Power'") || line.contains("'UPS Power'") {
        Some(false)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn snapshots_match_the_desktop_telemetry_contract() {
        let power = Power { on_battery: "true", locked: "false", thermal: "unknown" };
        let v = snapshot(3, 42, 1_700_000_000_000, "2023-11-14T22:13:20.000Z", power);
        assert_eq!(v["version"], 1);
        assert_eq!(v["type"], "desktopTelemetry");
        assert_eq!(v["sequence"], 3);
        assert_eq!(v["electronPid"], 42);
        let p = &v["power"];
        for (key, value) in [("source", "electron-main"), ("onBattery", "true"), ("locked", "false"), ("idle", "unknown"), ("lowPowerMode", "unknown"), ("thermalState", "unknown"), ("updatedAt", "2023-11-14T22:13:20.000Z")] {
            assert_eq!(p[key], value, "{key}");
        }
        assert_eq!((p["suspended"].clone(), p["stale"].clone(), p["idleSeconds"].clone()), (false.into(), false.into(), serde_json::Value::Null));
        assert!(v["speedLimitPercent"].is_null());
        assert_eq!(v["electronProcesses"], serde_json::json!([]));
        assert!(!v.to_string().contains('\n'), "one NDJSON line");
    }

    #[test]
    fn reads_battery_power_as_electron_reports_it() {
        let s = |v: &[(&str, &str)]| v.iter().map(|(a, b)| (a.to_string(), b.to_string())).collect::<Vec<_>>();
        assert_eq!(linux_on_battery(&s(&[])), Some(false));
        assert_eq!(linux_on_battery(&s(&[("Mains", "1")])), Some(false));
        assert_eq!(linux_on_battery(&s(&[("Battery", ""), ("Mains", "0")])), Some(true));
        assert_eq!(linux_on_battery(&s(&[("Battery", ""), ("Mains", "0"), ("USB", "1")])), Some(false));
        assert_eq!(linux_on_battery(&s(&[("Battery", "")])), None);
        assert_eq!(pmset_on_battery("Now drawing from 'Battery Power'\n -InternalBattery-0"), Some(true));
        assert_eq!(pmset_on_battery("Now drawing from 'AC Power'\n"), Some(false));
        assert_eq!(pmset_on_battery(""), None);
    }
}
