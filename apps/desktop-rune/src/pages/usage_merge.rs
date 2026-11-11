//! mergeUsage (packages/shared/src/usageMerge.ts): the selected environments' usage summaries as
//! one set of buckets, counting a transcript directory several environments read only once.

use std::collections::{HashMap, HashSet};

use serde_json::{Value, json};

use crate::data::{iso_ms, s as field};

/// USAGE_CONTRACT_VERSION: the summary shape this client reads.
pub const CONTRACT_VERSION: i64 = 9;
const COMPATIBLE_SINCE: i64 = 4;

/// One environment's answer (EnvironmentUsage).
pub struct Usage<'a> {
    pub id: &'a str,
    pub label: &'a str,
    pub summary: &'a Value,
}

/// UsageContractMismatch: `server_behind` when the environment runs the older contract.
#[derive(Clone, Debug, PartialEq)]
pub struct Mismatch {
    pub environment: String,
    pub server_behind: bool,
}

#[derive(Default)]
pub struct Merged {
    /// Every counted bucket, from whichever environment owns its directory.
    pub buckets: Vec<Value>,
    /// Distinct sessions per provider, in the order providers first contribute.
    pub sessions: Vec<(String, i64)>,
    pub duplicates: Vec<String>,
    pub mismatches: Vec<Mismatch>,
}

/// isCompatibleUsageContractVersion
pub fn compatible(version: i64, expected: i64) -> bool {
    version >= COMPATIBLE_SINCE && version <= expected
}

fn list<'a>(v: &'a Value, key: &str) -> &'a [Value] {
    v[key].as_array().map(Vec::as_slice).unwrap_or(&[])
}

fn fingerprint_key(source: &Value) -> String {
    let fp = &source["fingerprint"];
    json!([fp["hostId"], fp["provider"], fp["resolvedHomePath"], fp["volumeId"]]).to_string()
}

fn bucket_key(bucket: &Value) -> String {
    json!([bucket["day"], bucket["hourStart"], bucket["provider"], bucket["model"]]).to_string()
}

fn read_at(summary: &Value) -> Option<i64> {
    summary["readAt"].as_str().and_then(iso_ms)
}

/// bucketsForSource: the positions of the buckets one source's directory produced.
fn buckets_for_source(summary: &Value, source: &Value) -> Vec<usize> {
    let fp = &source["fingerprint"];
    let alone = list(summary, "sources").iter().filter(|s| s["fingerprint"]["provider"] == fp["provider"]).count() == 1;
    list(summary, "buckets")
        .iter()
        .enumerate()
        .filter(|(_, b)| b["provider"] == fp["provider"] && (b["sourcePath"] == fp["resolvedHomePath"] || (b["sourcePath"].is_null() && alone)))
        .map(|(n, _)| n)
        .collect()
}

#[derive(Default)]
struct Claims {
    /// Fingerprint to the owning environment and the source's position in its summary.
    owner: HashMap<String, (usize, usize)>,
    supplemental: HashMap<usize, HashSet<usize>>,
    sessions: HashMap<String, i64>,
    duplicates: Vec<String>,
}

/// claimSources: the newest complete scan owns a directory; a newer partial scan adds only cells
/// the owner lacks.
fn claim_sources(envs: &[&Usage]) -> Claims {
    let mut claims = Claims::default();
    let mut ordered: Vec<usize> = (0..envs.len()).collect();
    ordered.sort_by(|&a, &b| read_at(envs[b].summary).unwrap_or(0).cmp(&read_at(envs[a].summary).unwrap_or(0)).then_with(|| envs[a].id.cmp(envs[b].id)));
    for status in ["ok", "partial", "failed"] {
        for &e in &ordered {
            for (n, source) in list(envs[e].summary, "sources").iter().enumerate() {
                if source["status"] != status {
                    continue;
                }
                let key = fingerprint_key(source);
                if claims.owner.contains_key(&key) {
                    claims.duplicates.push(format!("{}: {}", envs[e].label, field(&source["fingerprint"], "resolvedHomePath")));
                    continue;
                }
                claims.owner.insert(key.clone(), (e, n));
                claims.sessions.insert(key, source["distinctSessions"].as_i64().unwrap_or(0));
            }
        }
    }
    let mut seen: HashMap<String, HashSet<String>> = HashMap::new();
    for &e in &ordered {
        let summary = envs[e].summary;
        for source in list(summary, "sources") {
            if source["status"] != "partial" {
                continue;
            }
            let key = fingerprint_key(source);
            let Some(&(o, at)) = claims.owner.get(&key) else { continue };
            let (owner, owner_source) = (envs[o].summary, &list(envs[o].summary, "sources")[at]);
            let older = matches!((read_at(summary), read_at(owner)), (Some(a), Some(b)) if a <= b);
            if owner_source["status"] != "ok" || older || summary["modelFilter"] != owner["modelFilter"] {
                continue;
            }
            let cells = seen.entry(key.clone()).or_insert_with(|| buckets_for_source(owner, owner_source).into_iter().map(|n| bucket_key(&list(owner, "buckets")[n])).collect());
            let mut added = false;
            for n in buckets_for_source(summary, source) {
                if cells.insert(bucket_key(&list(summary, "buckets")[n])) {
                    claims.supplemental.entry(e).or_default().insert(n);
                    added = true;
                }
            }
            if added {
                let sessions = claims.sessions.entry(key).or_default();
                *sessions = (*sessions).max(source["distinctSessions"].as_i64().unwrap_or(0));
            }
        }
    }
    claims
}

/// ownedContribution: the buckets and per-provider sessions of the directories `e` owns.
fn owned_contribution(e: usize, env: &Usage, claims: &Claims) -> (Vec<Value>, Vec<(String, i64)>) {
    let mut providers: HashSet<String> = HashSet::new();
    let mut sources: HashSet<(String, String)> = HashSet::new();
    let mut sessions: Vec<(String, i64)> = Vec::new();
    for source in list(env.summary, "sources") {
        let key = fingerprint_key(source);
        if source["status"] == "missing" || claims.owner.get(&key).map(|o| o.0) != Some(e) {
            continue;
        }
        let fp = &source["fingerprint"];
        let provider = field(fp, "provider").to_string();
        providers.insert(provider.clone());
        sources.insert((provider.clone(), field(fp, "resolvedHomePath").to_string()));
        let n = claims.sessions.get(&key).copied().unwrap_or_else(|| source["distinctSessions"].as_i64().unwrap_or(0));
        match sessions.iter_mut().find(|p| p.0 == provider) {
            Some(p) => p.1 += n,
            None => sessions.push((provider, n)),
        }
    }
    let supplemental = claims.supplemental.get(&e);
    let buckets = list(env.summary, "buckets")
        .iter()
        .enumerate()
        .filter(|(n, b)| {
            supplemental.is_some_and(|s| s.contains(n))
                || match b["sourcePath"].as_str() {
                    None => providers.contains(field(b, "provider")),
                    Some(path) => sources.contains(&(field(b, "provider").to_string(), path.to_string())),
                }
        })
        .map(|(_, b)| b.clone())
        .collect();
    (buckets, sessions)
}

/// mergeUsage: environments on an incompatible contract are left out and reported instead.
pub fn merge_usage(envs: &[Usage], expected: i64) -> Merged {
    let mut merged = Merged::default();
    let mut current = Vec::new();
    for env in envs {
        let version = env.summary["contractVersion"].as_i64().unwrap_or(0);
        if compatible(version, expected) {
            current.push(env);
        } else {
            merged.mismatches.push(Mismatch { environment: env.id.to_string(), server_behind: version < expected });
        }
    }
    let claims = claim_sources(&current);
    for (e, env) in current.iter().enumerate() {
        let (buckets, sessions) = owned_contribution(e, env, &claims);
        merged.buckets.extend(buckets);
        for (provider, n) in sessions {
            match merged.sessions.iter_mut().find(|p| p.0 == provider) {
                Some(p) => p.1 += n,
                None => merged.sessions.push((provider, n)),
            }
        }
    }
    merged.duplicates = claims.duplicates;
    merged
}

/// formatUsageContractMismatch
pub fn mismatch_line(label: &str, mismatch: &Mismatch) -> String {
    if mismatch.server_behind {
        format!("{label} runs an older server version and is excluded from totals.")
    } else {
        format!("This client is older than the server on {label}; its usage is excluded from totals.")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bucket(over: Value) -> Value {
        let mut b = json!({
            "day": "2026-08-07", "provider": "claude", "model": "claude-fable-5", "apiProvider": "",
            "totals": {"uncachedInputTokens": 100, "cachedInputTokens": 1000, "cacheCreationTokens": 10, "outputTokens": 50, "reasoningTokens": 0},
            "costUsd": 10.0, "cacheSavingsUsd": 2.0, "costSource": "modelPriced", "pricedAs": null, "records": 5, "unpricedRecords": 0, "sessions": 1,
        });
        for (k, v) in over.as_object().into_iter().flatten() {
            b[k.as_str()] = v.clone();
        }
        b
    }

    fn source(provider: &str, host: &str, home: &str) -> Value {
        json!({
            "fingerprint": {"hostId": host, "provider": provider, "resolvedHomePath": home, "volumeId": format!("vol-{host}")},
            "status": "ok", "scannedFiles": 1, "skippedFiles": 0, "malformedRecords": 0, "distinctSessions": 1, "message": null,
        })
    }

    fn summary(buckets: Vec<Value>, sources: Vec<Value>) -> Value {
        json!({"contractVersion": CONTRACT_VERSION, "readAt": "2026-08-07T00:00:00.000Z", "sinceDay": "2026-08-01", "untilDay": "2026-08-31", "buckets": buckets, "sources": sources})
    }

    fn partial(mut summary: Value, read_at: &str) -> Value {
        summary["readAt"] = json!(read_at);
        for s in summary["sources"].as_array_mut().into_iter().flatten() {
            s["status"] = json!("partial");
        }
        summary
    }

    fn merge(envs: &[(&str, &Value)]) -> Merged {
        let usages: Vec<Usage> = envs.iter().map(|&(id, summary)| Usage { id, label: id, summary }).collect();
        merge_usage(&usages, CONTRACT_VERSION)
    }

    fn sum(m: &Merged, key: &str) -> f64 {
        m.buckets.iter().map(|b| b[key].as_f64().unwrap_or(0.0)).sum()
    }

    fn sessions(m: &Merged) -> i64 {
        m.sessions.iter().map(|p| p.1).sum()
    }

    fn cost_by_provider(m: &Merged) -> Vec<(String, f64)> {
        let mut out: Vec<(String, f64)> = Vec::new();
        for b in &m.buckets {
            let provider = field(b, "provider").to_string();
            let cost = b["costUsd"].as_f64().unwrap_or(0.0);
            match out.iter_mut().find(|p| p.0 == provider) {
                Some(p) => p.1 += cost,
                None => out.push((provider, cost)),
            }
        }
        out.sort_by(|a, b| a.0.cmp(&b.0));
        out
    }

    #[test]
    fn a_cursor_account_counts_once_across_servers_keeping_their_other_providers() {
        let mut account = source("cursor", "cursor.com", "cursor-account:account-hash");
        account["fingerprint"]["volumeId"] = json!("account-hash");
        let mac = summary(vec![bucket(json!({"provider": "cursor", "sourcePath": "cursor-account:account-hash"}))], vec![account.clone()]);
        let linux = summary(
            vec![bucket(json!({"provider": "cursor", "sourcePath": "cursor-account:account-hash"})), bucket(json!({"provider": "opencode", "sourcePath": "/opencode"}))],
            vec![account, source("opencode", "linux", "/opencode")],
        );
        let m = merge(&[("mac", &mac), ("linux", &linux)]);
        assert_eq!(cost_by_provider(&m), [("cursor".to_string(), 10.0), ("opencode".to_string(), 10.0)]);
        assert_eq!(m.duplicates.len(), 1);
    }

    #[test]
    fn a_shared_transcript_directory_counts_once() {
        let shared = summary(vec![bucket(json!({}))], vec![source("claude", "mac", "/home/theo/.claude")]);
        let m = merge(&[("env-a", &shared), ("env-b", &shared)]);
        assert_eq!((sum(&m, "costUsd"), sum(&m, "records"), sessions(&m)), (10.0, 5.0, 1));
        assert_eq!(m.duplicates.len(), 1);
    }

    #[test]
    fn only_the_duplicated_provider_drops() {
        let claude = source("claude", "mac", "/home/theo/.claude");
        let a = summary(vec![bucket(json!({}))], vec![claude.clone()]);
        let b = summary(vec![bucket(json!({})), bucket(json!({"provider": "codex", "model": "gpt-5.6-sol", "costUsd": 4.0}))], vec![claude, source("codex", "mac", "/home/theo/.codex")]);
        let m = merge(&[("env-a", &a), ("env-b", &b)]);
        assert_eq!(sum(&m, "costUsd"), 14.0);
        assert_eq!(m.sessions, [("claude".to_string(), 1), ("codex".to_string(), 1)]);
    }

    #[test]
    fn overlapping_roots_count_once_keeping_each_unique_root() {
        let usage = |path: &str, cost: f64| bucket(json!({"provider": "opencode", "sourcePath": path, "costUsd": cost}));
        let a = summary(vec![usage("/shared", 10.0), usage("/a", 2.0)], vec![source("opencode", "host", "/shared"), source("opencode", "host", "/a")]);
        let b = summary(vec![usage("/shared", 10.0), usage("/b", 3.0)], vec![source("opencode", "host", "/shared"), source("opencode", "host", "/b")]);
        let m = merge(&[("env-a", &a), ("env-b", &b)]);
        assert_eq!((sum(&m, "costUsd"), sessions(&m)), (15.0, 3));
    }

    #[test]
    fn the_newest_scan_of_a_shared_directory_wins() {
        let claude = source("claude", "mac", "/home/theo/.claude");
        let a = summary(vec![bucket(json!({"costUsd": 4.0, "records": 2}))], vec![claude.clone()]);
        let mut b = summary(vec![bucket(json!({}))], vec![claude]);
        b["readAt"] = json!("2026-08-07T01:00:00.000Z");
        for envs in [[("env-a", &a), ("env-b", &b)], [("env-b", &b), ("env-a", &a)]] {
            let m = merge(&envs);
            assert_eq!((sum(&m, "costUsd"), sum(&m, "records"), sessions(&m)), (10.0, 5.0, 1));
            assert_eq!(m.duplicates, ["env-a: /home/theo/.claude"]);
        }
    }

    #[test]
    fn a_complete_scan_beats_a_newer_partial_one() {
        let claude = source("claude", "mac", "/home/theo/.claude");
        let new = partial(summary(vec![bucket(json!({"costUsd": 4.0, "records": 2}))], vec![claude.clone()]), "2026-08-07T01:00:00.000Z");
        let old = summary(vec![bucket(json!({}))], vec![claude]);
        for envs in [[("new", &new), ("old", &old)], [("old", &old), ("new", &new)]] {
            let m = merge(&envs);
            assert_eq!(sum(&m, "costUsd"), 10.0);
            assert_eq!(m.duplicates, ["new: /home/theo/.claude"]);
        }
        assert_eq!(sum(&merge(&[("new", &new)]), "costUsd"), 4.0);
    }

    #[test]
    fn a_later_partial_scan_adds_only_new_cells() {
        let claude = source("claude", "mac", "/home/theo/.claude");
        let mut old = summary(vec![bucket(json!({}))], vec![claude.clone()]);
        old["contractVersion"] = json!(COMPATIBLE_SINCE);
        let mut busier = claude;
        busier["distinctSessions"] = json!(2);
        let new = partial(
            summary(
                vec![
                    bucket(json!({"sourcePath": "/home/theo/.claude", "costUsd": 4.0, "records": 2})),
                    bucket(json!({"day": "2026-08-08", "sourcePath": "/home/theo/.claude", "costUsd": 3.0, "records": 1})),
                ],
                vec![busier],
            ),
            "2026-08-08T01:00:00.000Z",
        );
        for envs in [[("old", &old), ("new", &new)], [("new", &new), ("old", &old)]] {
            let m = merge(&envs);
            assert_eq!((sum(&m, "costUsd"), sum(&m, "records"), sessions(&m)), (13.0, 6.0, 2));
            assert_eq!(m.duplicates, ["new: /home/theo/.claude"]);
        }
        let mut filtered = new.clone();
        filtered["modelFilter"] = json!({"exclude": [{"model": "claude-fable-5"}]});
        let m = merge(&[("old", &old), ("new", &filtered)]);
        assert_eq!((sum(&m, "costUsd"), sessions(&m)), (10.0, 1));
    }

    #[test]
    fn a_larger_partial_cell_does_not_replace_a_complete_one() {
        let claude = source("claude", "mac", "/home/theo/.claude");
        let old = summary(vec![bucket(json!({}))], vec![claude.clone()]);
        let new = partial(summary(vec![bucket(json!({"costUsd": 4.0, "records": 6}))], vec![claude]), "2026-08-07T01:00:00.000Z");
        let m = merge(&[("old", &old), ("new", &new)]);
        assert_eq!((sum(&m, "costUsd"), sum(&m, "records"), sessions(&m)), (10.0, 5.0, 1));
    }

    #[test]
    fn incompatible_contracts_are_left_out_and_reported() {
        let a = summary(vec![bucket(json!({}))], vec![source("claude", "mac", "/a")]);
        let mut older = summary(vec![bucket(json!({}))], vec![source("claude", "linux", "/b")]);
        older["contractVersion"] = json!(COMPATIBLE_SINCE - 1);
        let m = merge(&[("env-a", &a), ("env-b", &older)]);
        assert_eq!(sum(&m, "costUsd"), 10.0);
        assert_eq!(m.mismatches, [Mismatch { environment: "env-b".into(), server_behind: true }]);
        assert_eq!(mismatch_line("Studio", &m.mismatches[0]), "Studio runs an older server version and is excluded from totals.");

        let mut newer = a.clone();
        newer["contractVersion"] = json!(CONTRACT_VERSION + 1);
        let m = merge(&[("env-a", &newer)]);
        assert!(m.buckets.is_empty());
        assert_eq!(m.mismatches, [Mismatch { environment: "env-a".into(), server_behind: false }]);
        assert_eq!(mismatch_line("Studio", &m.mismatches[0]), "This client is older than the server on Studio; its usage is excluded from totals.");

        let mut previous = summary(vec![bucket(json!({"costUsd": 4.0, "provider": "codex", "model": "gpt-5.6-sol"}))], vec![source("codex", "linux", "/b")]);
        previous["contractVersion"] = json!(CONTRACT_VERSION - 1);
        let m = merge(&[("env-a", &a), ("env-b", &previous)]);
        assert_eq!(sum(&m, "costUsd"), 14.0);
        assert!(m.mismatches.is_empty());
    }

    #[test]
    fn the_volume_keeps_two_machines_apart_when_host_and_home_collide() {
        let on = |volume: &str| {
            let mut s = source("claude", "mac", "/Users/theo/.claude");
            s["fingerprint"]["volumeId"] = json!(volume);
            summary(vec![bucket(json!({}))], vec![s])
        };
        let (a, b) = (on("16777220:1234"), on("16777221:9999"));
        let m = merge(&[("env-a", &a), ("env-b", &b)]);
        assert_eq!(sum(&m, "costUsd"), 20.0);
        assert!(m.duplicates.is_empty());
    }

    #[test]
    fn sessions_come_from_each_directorys_distinct_count() {
        let a = summary(vec![bucket(json!({"day": "2026-08-06"})), bucket(json!({"day": "2026-08-07"}))], vec![source("claude", "mac", "/a/.claude")]);
        assert_eq!(merge(&[("env-a", &a)]).sessions, [("claude".to_string(), 1)]);
        assert!(merge(&[]).buckets.is_empty());
    }
}
