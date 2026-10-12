//! Usage metering: Claude 5-hour window tokens/cost and Codex quota parsing.

use crate::interface::diagnostics::SourceHealth;
use crate::{codex_data_root, epoch_secs, home_dir, list_newest_files, read_tail, UsageInfo};
use serde_json::Value;
use std::collections::HashMap;
use std::sync::{Mutex, OnceLock};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

pub(crate) fn epoch_millis() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(0)
}

pub(crate) fn days_from_civil(y: i64, m: u32, d: u32) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = m as i64 + if m > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + d as i64 - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

pub(crate) fn parse_rfc3339_secs(s: &str) -> Option<u64> {
    let (date, rest) = s.split_once('T')?;
    let mut dp = date.split('-');
    let year: i64 = dp.next()?.parse().ok()?;
    let month: i64 = dp.next()?.parse().ok()?;
    let day: i64 = dp.next()?.parse().ok()?;
    let time = rest
        .strip_suffix('Z')
        .unwrap_or_else(|| rest.split(['+', '-']).next().unwrap_or(rest));
    let mut tp = time.split(':');
    let hour: i64 = tp.next()?.parse().ok()?;
    let minute: i64 = tp.next()?.parse().ok()?;
    let second: i64 = tp.next()?.split('.').next()?.parse().ok()?;
    let days = days_from_civil(year, month as u32, day as u32);
    Some((days * 86400 + hour * 3600 + minute * 60 + second) as u64)
}

pub(crate) struct ClaudePrice {
    pub(crate) input: f64,
    pub(crate) output: f64,
    pub(crate) cache_read: f64,
    pub(crate) cache_write: f64,
}

pub(crate) fn claude_price(model: &str) -> Option<ClaudePrice> {
    let m = model.to_lowercase();
    let p = if m.contains("opus") {
        ClaudePrice {
            input: 15.0,
            output: 75.0,
            cache_read: 1.5,
            cache_write: 18.75,
        }
    } else if m.contains("sonnet") {
        ClaudePrice {
            input: 3.0,
            output: 15.0,
            cache_read: 0.3,
            cache_write: 3.75,
        }
    } else if m.contains("haiku") {
        ClaudePrice {
            input: 1.0,
            output: 5.0,
            cache_read: 0.1,
            cache_write: 1.25,
        }
    } else {
        return None;
    };
    Some(p)
}

pub(crate) struct ClaudeScan {
    pub(crate) found: bool,
    pub(crate) input: u64,
    pub(crate) output: u64,
    pub(crate) cache_read: u64,
    pub(crate) cache_write: u64,
    pub(crate) cost: f64,
    pub(crate) cost_known: bool,
}

pub(crate) fn scan_claude_text(text: &str, window_start: u64) -> ClaudeScan {
    let mut scan = ClaudeScan {
        found: false,
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        cost: 0.0,
        cost_known: false,
    };
    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if let Some(ts) = v
            .get("timestamp")
            .and_then(|t| t.as_str())
            .and_then(parse_rfc3339_secs)
        {
            if ts < window_start {
                break;
            }
        }
        let Some(msg) = v.get("message") else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("assistant") {
            continue;
        }
        let Some(usage) = msg.get("usage") else {
            continue;
        };
        let input = usage
            .get("input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let output = usage
            .get("output_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let cache_read = usage
            .get("cache_read_input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let cache_write = usage
            .get("cache_creation_input_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        scan.found = true;
        scan.input += input;
        scan.output += output;
        scan.cache_read += cache_read;
        scan.cache_write += cache_write;
        if let Some(model) = msg.get("model").and_then(|m| m.as_str()) {
            if let Some(p) = claude_price(model) {
                scan.cost += input as f64 / 1e6 * p.input
                    + output as f64 / 1e6 * p.output
                    + cache_read as f64 / 1e6 * p.cache_read
                    + cache_write as f64 / 1e6 * p.cache_write;
                scan.cost_known = true;
            }
        }
    }
    scan
}

pub(crate) const CLAUDE_WINDOW_SECS: u64 = 5 * 3600;

pub(crate) fn claude_usage() -> Option<UsageInfo> {
    let root = home_dir()?.join(".claude").join("projects");
    let files = list_newest_files(&root, 4, "jsonl", 8);
    if files.is_empty() {
        return None;
    }
    let window_start = epoch_secs().saturating_sub(CLAUDE_WINDOW_SECS);
    let mut total = ClaudeScan {
        found: false,
        input: 0,
        output: 0,
        cache_read: 0,
        cache_write: 0,
        cost: 0.0,
        cost_known: false,
    };
    for path in &files {
        let text = read_tail(path, 512 * 1024);
        let scan = scan_claude_text(&text, window_start);
        total.found |= scan.found;
        total.input += scan.input;
        total.output += scan.output;
        total.cache_read += scan.cache_read;
        total.cache_write += scan.cache_write;
        total.cost += scan.cost;
        total.cost_known |= scan.cost_known;
    }
    Some(UsageInfo {
        tokens_total: total.input + total.output + total.cache_read + total.cache_write,
        tokens_output: total.output,
        cost_usd: if total.cost_known {
            Some((total.cost * 100.0).round() / 100.0)
        } else {
            None
        },
        used_percent: None,
        window_secs: CLAUDE_WINDOW_SECS,
        resets_at_secs: None,
        credits: None,
        unlimited: None,
        stale: !total.found,
    })
}

pub(crate) fn scan_codex_text(text: &str, now: u64) -> Option<UsageInfo> {
    let mut best: Option<(u64, UsageInfo)> = None;
    for line in text.lines().rev() {
        let Ok(v) = serde_json::from_str::<Value>(line) else {
            continue;
        };
        if v.get("type").and_then(|t| t.as_str()) != Some("event_msg") {
            continue;
        }
        let Some(payload) = v.get("payload") else {
            continue;
        };
        if payload.get("type").and_then(|t| t.as_str()) != Some("token_count") {
            continue;
        }
        let Some(info) = payload.get("info") else {
            continue;
        };
        let tokens_total = info
            .pointer("/total_token_usage/total_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let tokens_output = info
            .pointer("/total_token_usage/output_tokens")
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let rl = payload.get("rate_limits");
        let primary = rl.and_then(|r| r.get("primary"));
        let used_percent = primary
            .and_then(|p| p.get("used_percent"))
            .and_then(|x| x.as_f64())
            .map(|x| x as f32);
        let resets_at = primary
            .and_then(|p| p.get("resets_at"))
            .and_then(|x| x.as_u64());
        let window_minutes = primary
            .and_then(|p| p.get("window_minutes"))
            .and_then(|x| x.as_u64())
            .unwrap_or(0);
        let credits = rl.and_then(|r| r.get("credits"));
        let has_credits = credits
            .and_then(|c| c.get("has_credits"))
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        let unlimited = credits
            .and_then(|c| c.get("unlimited"))
            .and_then(|x| x.as_bool())
            .unwrap_or(false);
        let balance = credits
            .and_then(|c| c.get("balance"))
            .and_then(|x| x.as_f64());
        let mut info_out = UsageInfo {
            tokens_total,
            tokens_output,
            cost_usd: None,
            used_percent,
            window_secs: window_minutes * 60,
            resets_at_secs: resets_at,
            credits: if has_credits { balance } else { None },
            unlimited: Some(unlimited),
            stale: false,
        };
        if resets_at.map(|r| r <= now).unwrap_or(false) {
            info_out.used_percent = Some(0.0);
            info_out.stale = true;
        }
        let key = resets_at.unwrap_or(0);
        if best.as_ref().map(|(k, _)| key > *k).unwrap_or(true) {
            best = Some((key, info_out));
        }
        break;
    }
    best.map(|(_, info)| info)
}

pub(crate) fn codex_usage() -> Option<UsageInfo> {
    let root = codex_data_root()?.join("sessions");
    let files = list_newest_files(&root, 5, "jsonl", 6);
    if files.is_empty() {
        return None;
    }
    let now = epoch_secs();
    let mut best: Option<(u64, UsageInfo)> = None;
    for path in &files {
        let text = read_tail(path, 256 * 1024);
        if let Some(info) = scan_codex_text(&text, now) {
            let key = info.resets_at_secs.unwrap_or(0);
            if best.as_ref().map(|(k, _)| key > *k).unwrap_or(true) {
                best = Some((key, info));
            }
        }
    }
    best.map(|(_, info)| info)
}

static USAGE_CACHE: OnceLock<Mutex<HashMap<String, (Instant, Option<UsageInfo>)>>> =
    OnceLock::new();

pub(crate) fn usage_for(name: &str) -> Option<UsageInfo> {
    let cache = USAGE_CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    if let Ok(guard) = cache.lock() {
        if let Some((at, value)) = guard.get(name) {
            if at.elapsed().as_secs() < 30 {
                return value.clone();
            }
        }
    }
    let value = match name {
        "Claude Code" => claude_usage(),
        "Codex CLI" => codex_usage(),
        _ => None,
    };
    if let Ok(mut guard) = cache.lock() {
        guard.insert(name.to_string(), (Instant::now(), value.clone()));
    }
    value
}

static USAGE_HEALTH_CACHE: OnceLock<Mutex<Option<(Instant, Vec<SourceHealth>)>>> = OnceLock::new();

pub(crate) fn usage_source_health() -> Vec<SourceHealth> {
    let cache = USAGE_HEALTH_CACHE.get_or_init(|| Mutex::new(None));
    if let Ok(guard) = cache.lock() {
        if let Some((at, value)) = guard.as_ref() {
            if at.elapsed().as_secs() < 30 {
                return value.clone();
            }
        }
    }
    let value = vec![claude_usage_health(), codex_usage_health()];
    if let Ok(mut guard) = cache.lock() {
        *guard = Some((Instant::now(), value.clone()));
    }
    value
}

fn claude_usage_health() -> SourceHealth {
    let records = home_dir()
        .map(|home| list_newest_files(&home.join(".claude").join("projects"), 4, "jsonl", 8).len());
    let Some(records) = records else {
        return SourceHealth {
            source: "claude_usage".into(),
            status: "unavailable".into(),
            records: 0,
        };
    };
    if records == 0 {
        return SourceHealth {
            source: "claude_usage".into(),
            status: "no_records".into(),
            records: 0,
        };
    }
    SourceHealth {
        source: "claude_usage".into(),
        status: claude_usage_status(records, usage_for("Claude Code")).into(),
        records,
    }
}

fn codex_usage_health() -> SourceHealth {
    let records = codex_data_root()
        .map(|root| list_newest_files(&root.join("sessions"), 5, "jsonl", 6).len());
    let Some(records) = records else {
        return SourceHealth {
            source: "codex_usage".into(),
            status: "unavailable".into(),
            records: 0,
        };
    };
    if records == 0 {
        return SourceHealth {
            source: "codex_usage".into(),
            status: "no_records".into(),
            records: 0,
        };
    }
    SourceHealth {
        source: "codex_usage".into(),
        status: codex_usage_status(records, usage_for("Codex CLI")).into(),
        records,
    }
}

pub(crate) fn claude_usage_status(records: usize, usage: Option<UsageInfo>) -> &'static str {
    match usage {
        Some(info) if !info.stale => "ok",
        // Claude marks stale when no usage event falls inside the window.
        // Idle time and a changed record format look identical here, so the
        // status reports the missing events instead of claiming either cause.
        Some(_) => "no_usage_events",
        None if records > 0 => "unavailable",
        None => "no_records",
    }
}

pub(crate) fn codex_usage_status(records: usize, usage: Option<UsageInfo>) -> &'static str {
    match usage {
        // A stale Codex reading only means the quota window expired; the
        // token_count event itself parsed, so the source is healthy.
        Some(_) => "ok",
        None if records > 0 => "no_usage_events",
        None => "no_records",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(stale: bool) -> UsageInfo {
        UsageInfo {
            tokens_total: 10,
            tokens_output: 5,
            cost_usd: None,
            used_percent: None,
            window_secs: 0,
            resets_at_secs: None,
            credits: None,
            unlimited: None,
            stale,
        }
    }

    #[test]
    fn claude_status_reports_missing_events_without_claiming_a_cause() {
        assert_eq!(claude_usage_status(3, Some(info(false))), "ok");
        assert_eq!(claude_usage_status(3, Some(info(true))), "no_usage_events");
        assert_eq!(claude_usage_status(0, None), "no_records");
        assert_eq!(claude_usage_status(3, None), "unavailable");
    }

    #[test]
    fn claude_price_table_covers_the_known_model_families_case_insensitively() {
        let opus = claude_price("claude-opus-4-1").expect("opus price");
        assert_eq!(
            (opus.input, opus.output, opus.cache_read, opus.cache_write),
            (15.0, 75.0, 1.5, 18.75)
        );
        let sonnet = claude_price("claude-sonnet-4-5").expect("sonnet price");
        assert_eq!(
            (
                sonnet.input,
                sonnet.output,
                sonnet.cache_read,
                sonnet.cache_write
            ),
            (3.0, 15.0, 0.3, 3.75)
        );
        let haiku = claude_price("CLAUDE-HAIKU-4").expect("haiku price is case-insensitive");
        assert_eq!(
            (
                haiku.input,
                haiku.output,
                haiku.cache_read,
                haiku.cache_write
            ),
            (1.0, 5.0, 0.1, 1.25)
        );
        assert!(claude_price("deepseek-v4-pro").is_none());
    }

    #[test]
    fn codex_status_treats_an_expired_window_as_a_healthy_parse() {
        assert_eq!(codex_usage_status(3, Some(info(true))), "ok");
        assert_eq!(codex_usage_status(3, Some(info(false))), "ok");
        assert_eq!(codex_usage_status(3, None), "no_usage_events");
        assert_eq!(codex_usage_status(0, None), "no_records");
    }
}
