//! What an administrator's managed policy (`managedPolicy` in the server config) allows on the
//! Providers page: web `isManagedDriverAllowed`, `managedInstanceLocks` and
//! `disallowedProviderMessage`.
use crate::App;
use serde_json::Value;

/// The policy of the environment the page edits, `Null` when it is unmanaged.
pub fn of(s: &App) -> &Value {
    s.state().config.as_ref().map_or(&Value::Null, |c| &c["managedPolicy"])
}

/// Contracts `isManagedDriverAllowed`: an absent `allowedDrivers` allows every driver.
pub fn driver_allowed(policy: &Value, driver: &str) -> bool {
    match policy["limits"]["providers"]["allowedDrivers"].as_array() {
        Some(allowed) => allowed.iter().any(|d| d == driver),
        None => true,
    }
}

/// A policy that allows no provider leaves nothing to add.
pub fn can_add(policy: &Value) -> bool {
    policy["limits"]["providers"]["allowedDrivers"].as_array().is_none_or(|a| !a.is_empty())
}

/// Contracts `disallowedProviderMessage`, when the policy refuses `driver`.
pub fn driver_denial(policy: &Value, driver: &str, label: &str) -> Option<String> {
    if driver_allowed(policy, driver) {
        return None;
    }
    Some(match policy["organization"].as_str() {
        Some(org) => format!("{org}'s policy does not allow the {label} provider."),
        None => format!("Your organization policy does not allow the {label} provider."),
    })
}

/// Web `ManagedInstanceLocks`: what the policy pins on instances of one driver.
#[derive(Debug, Default, PartialEq)]
pub struct Locks {
    pub config: Vec<String>,
    pub environment: Vec<String>,
    pub whole_environment: bool,
    pub reason: String,
}

impl Locks {
    pub fn config_reason(&self, field: &str) -> Option<String> {
        self.config.iter().any(|k| k == field).then(|| self.reason.clone())
    }
    pub fn variable_reason(&self, name: &str) -> Option<String> {
        (self.whole_environment || self.environment.iter().any(|n| n == name)).then(|| self.reason.clone())
    }
}

/// Web `managedInstanceLocks`.
pub fn locks(policy: &Value, driver: &str) -> Option<Locks> {
    let paths: Vec<&str> =
        policy["limits"]["providers"]["lockedInstanceFields"][driver].as_array()?.iter().filter_map(Value::as_str).collect();
    let organization = policy["organization"].as_str()?;
    if paths.is_empty() {
        return None;
    }
    let under = |prefix: &str| paths.iter().filter_map(|p| p.strip_prefix(prefix)).map(str::to_owned).collect();
    Some(Locks {
        config: under("config."),
        environment: under("environment."),
        whole_environment: paths.contains(&"environment"),
        reason: format!("Managed by {organization}."),
    })
}

/// The reason a config field is pinned for `driver`, if it is.
pub fn config_reason(policy: &Value, driver: &str, field: &str) -> Option<String> {
    locks(policy, driver)?.config_reason(field)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn policy_limits_read_like_web() {
        let open = json!({"organization": "Acme"});
        assert!(driver_allowed(&open, "kiro") && can_add(&open) && locks(&open, "codex").is_none());
        assert!(driver_allowed(&Value::Null, "kiro") && can_add(&Value::Null));

        let policy = json!({"organization": "Acme", "limits": {"providers": {
            "allowedDrivers": ["codex", "acpRegistry"],
            "lockedInstanceFields": {"codex": ["config.binaryPath", "config.setupMode", "environment.OPENAI_API_KEY"], "cursor": ["environment"], "grok": []}
        }}});
        assert!(driver_allowed(&policy, "acpRegistry") && !driver_allowed(&policy, "claudeAgent"));
        assert_eq!(driver_denial(&policy, "claudeAgent", "Claude").as_deref(), Some("Acme's policy does not allow the Claude provider."));
        assert_eq!(driver_denial(&policy, "codex", "Codex"), None);
        assert!(!can_add(&json!({"organization": "Acme", "limits": {"providers": {"allowedDrivers": []}}})));

        let codex = locks(&policy, "codex").unwrap();
        assert_eq!(codex.config, ["binaryPath", "setupMode"]);
        assert_eq!(codex.variable_reason("OPENAI_API_KEY").as_deref(), Some("Managed by Acme."));
        assert_eq!(codex.variable_reason("OTHER"), None);
        assert_eq!(config_reason(&policy, "codex", "homePath"), None);
        let cursor = locks(&policy, "cursor").unwrap();
        assert!(cursor.whole_environment && cursor.variable_reason("CURSOR_API_KEY").is_some());
        assert!(locks(&policy, "grok").is_none(), "an empty list pins nothing");
    }
}
