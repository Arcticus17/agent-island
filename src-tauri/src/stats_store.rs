//! Stats persistence: aggregate/daily stat files and terminal-hash migration.

use crate::{home_dir, AgentStats};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) const RECENT_TERMINAL_HASH_CAPACITY: usize = 128;

pub(crate) fn terminal_key_hash(transition_key: &str) -> String {
    format!("{:x}", Sha256::digest(transition_key.as_bytes()))
}

pub(crate) fn remember_terminal_hash(stats: &mut AgentStats, transition_hash: String) {
    stats.recent_terminal_hashes.push(transition_hash);
    if stats.recent_terminal_hashes.len() > RECENT_TERMINAL_HASH_CAPACITY {
        stats.recent_terminal_hashes.remove(0);
    }
}

pub(crate) fn normalize_terminal_hashes(hashes: impl IntoIterator<Item = String>) -> Vec<String> {
    let mut normalized = Vec::new();
    for hash in hashes {
        if hash.len() == 64
            && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
            && !normalized.iter().any(|existing| existing == &hash)
        {
            normalized.push(hash);
        }
    }
    if normalized.len() > RECENT_TERMINAL_HASH_CAPACITY {
        normalized.drain(..normalized.len() - RECENT_TERMINAL_HASH_CAPACITY);
    }
    normalized
}

pub(crate) fn stats_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("stats.json")
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct StoredAgentStats {
    total_seconds: u64,
    error_count: u32,
    done_count: u32,
    last_status: Option<String>,
    recent_terminal_hashes: Vec<String>,
    last_transition_key: Option<String>,
}

impl From<StoredAgentStats> for AgentStats {
    fn from(stored: StoredAgentStats) -> Self {
        let legacy_hash = stored
            .last_transition_key
            .map(|key| terminal_key_hash(&key));
        let hashes = stored
            .recent_terminal_hashes
            .into_iter()
            .chain(legacy_hash)
            .collect::<Vec<_>>();
        Self {
            total_seconds: stored.total_seconds,
            error_count: stored.error_count,
            done_count: stored.done_count,
            last_status: stored.last_status,
            recent_terminal_hashes: normalize_terminal_hashes(hashes),
        }
    }
}

pub(crate) fn load_stats(path: &Path) -> HashMap<String, AgentStats> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str::<HashMap<String, StoredAgentStats>>(&s).ok())
        .map(|stored| {
            stored
                .into_iter()
                .map(|(agent, stats)| (agent, stats.into()))
                .collect()
        })
        .unwrap_or_default()
}

pub(crate) fn save_stats(stats: &HashMap<String, AgentStats>, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(stats).map_err(|e| e.to_string())?;
    fs::write(path, data).map_err(|e| e.to_string())
}

pub(crate) fn daily_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("stats-daily.json")
}

pub(crate) fn load_daily(path: &Path) -> HashMap<String, HashMap<String, AgentStats>> {
    fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default()
}

pub(crate) fn save_daily(
    daily: &HashMap<String, HashMap<String, AgentStats>>,
    path: &Path,
) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let data = serde_json::to_string_pretty(daily).map_err(|e| e.to_string())?;
    fs::write(path, data).map_err(|e| e.to_string())
}

pub(crate) fn date_key(secs: u64) -> String {
    let days = secs / 86400;
    let z = days as i64 + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y } as u32;
    format!("{y:04}-{m:02}-{d:02}")
}

pub(crate) fn today_key() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    date_key(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn date_key_buckets_epoch_seconds_into_utc_calendar_days() {
        assert_eq!(date_key(0), "1970-01-01");
        assert_eq!(date_key(86_400), "1970-01-02");
        // 20454 days after the epoch is 2026-01-01.
        assert_eq!(date_key(20_454 * 86_400), "2026-01-01");
        // 19783 days is 2024-03-01, the day after a leap-day February.
        assert_eq!(date_key(19_783 * 86_400), "2024-03-01");
    }

    #[test]
    fn stats_round_trip_through_disk_keeps_counters_and_hashes() {
        let path = std::env::temp_dir().join(format!(
            "agent-island-stats-roundtrip-{}.json",
            std::process::id()
        ));
        let mut stats = HashMap::new();
        let mut entry = AgentStats::default();
        entry.total_seconds = 42;
        entry.error_count = 2;
        entry.done_count = 3;
        entry.recent_terminal_hashes.push("a".repeat(64));
        stats.insert("Claude Code".to_string(), entry);

        save_stats(&stats, &path).expect("save should succeed");
        let loaded = load_stats(&path);
        let agent = loaded.get("Claude Code").expect("agent survives reload");
        assert_eq!(agent.total_seconds, 42);
        assert_eq!(agent.error_count, 2);
        assert_eq!(agent.done_count, 3);
        assert_eq!(agent.recent_terminal_hashes, vec!["a".repeat(64)]);

        fs::remove_file(&path).ok();
    }
}
