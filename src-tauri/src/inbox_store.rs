//! Persistent inbox for events that need a human.
//!
//! Before this module the island only had ephemeral notification/approval cards:
//! once the card was dismissed (or the app restarted) the event was gone. The
//! inbox keeps a small, local, ordered queue so "what did I miss while I was
//! away?" can be answered after the fact.
//!
//! Design constraints:
//! - Local only, no network.
//! - Atomic writes (temp + rename), BOM tolerant, corrupt file reads as empty.
//! - Bounded: at most 300 items, resolved items older than 7 days are pruned.
//! - Cheap: every hook event is a tiny JSON upsert, not a database.

use crate::*;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

pub(crate) const MAX_INBOX_ITEMS: usize = 300;
const RESOLVED_RETENTION_MS: u64 = 7 * 24 * 60 * 60 * 1000;
const DEDUPE_WINDOW_MS: u64 = 120_000;
const MAX_TITLE_CHARS: usize = 200;
const MAX_BODY_CHARS: usize = 4_000;

static INBOX_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub(crate) struct InboxItem {
    pub id: String,
    /// approval | notification | done | question | system
    pub kind: String,
    /// low | normal | high | critical
    pub severity: String,
    #[serde(default)]
    pub agent_id: String,
    #[serde(default)]
    pub agent_name: String,
    #[serde(default)]
    pub session_id: String,
    pub title: String,
    #[serde(default)]
    pub body: String,
    /// open | snoozed | resolved
    #[serde(default = "open_state")]
    pub state: String,
    pub created_at_ms: u64,
    #[serde(default)]
    pub snooze_until_ms: Option<u64>,
    #[serde(default)]
    pub resolved_at_ms: Option<u64>,
    /// Stable key used to merge repeated events (approval id, notification hash…).
    #[serde(default)]
    pub dedupe_key: String,
    /// Link to the approval card when kind == "approval".
    #[serde(default)]
    pub approval_id: Option<String>,
}

fn open_state() -> String {
    "open".to_string()
}

impl InboxItem {
    pub(crate) fn new(
        kind: &str,
        severity: &str,
        agent_id: &str,
        agent_name: &str,
        session_id: &str,
        title: &str,
        body: &str,
        dedupe_key: &str,
    ) -> Self {
        let now = epoch_millis();
        Self {
            id: new_inbox_id(now),
            kind: kind.to_string(),
            severity: severity.to_string(),
            agent_id: agent_id.to_string(),
            agent_name: agent_name.to_string(),
            session_id: session_id.to_string(),
            title: truncate_chars(title, MAX_TITLE_CHARS),
            body: truncate_chars(body, MAX_BODY_CHARS),
            state: "open".to_string(),
            created_at_ms: now,
            snooze_until_ms: None,
            resolved_at_ms: None,
            dedupe_key: dedupe_key.to_string(),
            approval_id: None,
        }
    }

    pub(crate) fn with_approval_id(mut self, approval_id: &str) -> Self {
        self.approval_id = Some(approval_id.to_string());
        self
    }
}

fn truncate_chars(text: &str, limit: usize) -> String {
    if text.chars().count() <= limit {
        return text.to_string();
    }
    let mut value: String = text.chars().take(limit).collect();
    value.push('…');
    value
}

pub(crate) fn inbox_path() -> PathBuf {
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".agent-island")
        .join("inbox.json")
}

pub(crate) fn new_inbox_id(now_ms: u64) -> String {
    let sequence = INBOX_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    format!("i{now_ms:x}{sequence:x}")
}

/// Read the inbox. Missing/corrupt/oversized files return an empty list rather
/// than preventing the island from starting.
pub(crate) fn load_inbox(path: &Path) -> Vec<InboxItem> {
    let raw = fs::read_to_string(path).ok();
    let Some(raw) = raw else {
        return Vec::new();
    };
    if raw.len() > 2 * 1024 * 1024 {
        return Vec::new();
    }
    let raw = raw.trim_start_matches('\u{feff}');
    let mut items: Vec<InboxItem> = serde_json::from_str(raw).unwrap_or_default();
    normalize_snoozed(&mut items, epoch_millis());
    prune_inbox(&mut items, epoch_millis());
    items
}

pub(crate) fn save_inbox(path: &Path, items: &[InboxItem]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|error| format!("创建收件箱目录失败：{error}"))?;
    }
    let body = serde_json::to_string_pretty(items).map_err(|error| error.to_string())?;
    let temp = path.with_extension("json.tmp");
    fs::write(&temp, body).map_err(|error| format!("写入收件箱失败：{error}"))?;
    fs::rename(&temp, path).map_err(|error| format!("替换收件箱失败：{error}"))?;
    Ok(())
}

/// Insert or refresh an item. Repeats with the same dedupe key inside the
/// window replace the existing item instead of creating a new row, so a noisy
/// agent cannot flood the inbox.
pub(crate) fn upsert_inbox(items: &mut Vec<InboxItem>, mut item: InboxItem, now_ms: u64) {
    if item.dedupe_key.is_empty() {
        item.dedupe_key = item.id.clone();
    }
    if let Some(existing) = items.iter_mut().find(|candidate| {
        candidate.dedupe_key == item.dedupe_key
            && candidate.state != "resolved"
            && now_ms.saturating_sub(candidate.created_at_ms) <= DEDUPE_WINDOW_MS
    }) {
        existing.created_at_ms = item.created_at_ms;
        existing.severity = item.severity;
        existing.agent_id = item.agent_id;
        existing.agent_name = item.agent_name;
        existing.session_id = item.session_id;
        existing.title = item.title;
        existing.body = item.body;
        existing.approval_id = item.approval_id;
        existing.state = "open".to_string();
        existing.snooze_until_ms = None;
        existing.resolved_at_ms = None;
        return;
    }
    items.push(item);
    prune_inbox(items, now_ms);
}

pub(crate) fn normalize_snoozed(items: &mut [InboxItem], now_ms: u64) {
    for item in items {
        if item.state == "snoozed" && item.snooze_until_ms.unwrap_or(0) <= now_ms {
            item.state = "open".to_string();
            item.snooze_until_ms = None;
        }
    }
}

pub(crate) fn prune_inbox(items: &mut Vec<InboxItem>, now_ms: u64) {
    items.retain(|item| {
        item.state != "resolved"
            || now_ms.saturating_sub(item.resolved_at_ms.unwrap_or(0)) <= RESOLVED_RETENTION_MS
    });
    if items.len() > MAX_INBOX_ITEMS {
        items.sort_by_key(|item| item.created_at_ms);
        let drop_count = items.len() - MAX_INBOX_ITEMS;
        items.drain(..drop_count);
    }
}

pub(crate) fn resolve_inbox(items: &mut [InboxItem], id: &str, now_ms: u64) -> bool {
    let Some(item) = items.iter_mut().find(|candidate| candidate.id == id) else {
        return false;
    };
    item.state = "resolved".to_string();
    item.resolved_at_ms = Some(now_ms);
    item.snooze_until_ms = None;
    true
}

pub(crate) fn resolve_inbox_by_approval(
    items: &mut [InboxItem],
    approval_id: &str,
    now_ms: u64,
) -> bool {
    let Some(item) = items
        .iter_mut()
        .find(|candidate| candidate.approval_id.as_deref() == Some(approval_id))
    else {
        return false;
    };
    item.state = "resolved".to_string();
    item.resolved_at_ms = Some(now_ms);
    item.snooze_until_ms = None;
    true
}

pub(crate) fn snooze_inbox(items: &mut [InboxItem], id: &str, until_ms: u64) -> bool {
    let Some(item) = items.iter_mut().find(|candidate| candidate.id == id) else {
        return false;
    };
    if item.state == "resolved" {
        return false;
    }
    item.state = "snoozed".to_string();
    item.snooze_until_ms = Some(until_ms);
    true
}

pub(crate) fn sort_inbox(items: &mut [InboxItem]) {
    items.sort_by(|left, right| {
        let left_resolved = left.state == "resolved";
        let right_resolved = right.state == "resolved";
        left_resolved
            .cmp(&right_resolved)
            .then_with(|| right.created_at_ms.cmp(&left.created_at_ms))
    });
}

fn snapshot(state: &tauri::State<AppState>) -> Vec<InboxItem> {
    let now = epoch_millis();
    let mut items = state.inbox.lock_recover().clone();
    normalize_snoozed(&mut items, now);
    sort_inbox(&mut items);
    items
}

#[tauri::command]
pub(crate) fn list_inbox(state: tauri::State<AppState>) -> Vec<InboxItem> {
    snapshot(&state)
}

#[tauri::command]
pub(crate) fn resolve_inbox_item(
    id: String,
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<Vec<InboxItem>, String> {
    let now = epoch_millis();
    let changed = {
        let mut items = state.inbox.lock_recover();
        let changed = resolve_inbox(&mut items, &id, now);
        if changed {
            prune_inbox(&mut items, now);
            save_inbox(&state.inbox_path, &items)?;
        }
        changed
    };
    if changed {
        use tauri::Emitter;
        let _ = app.emit("inbox-changed", ());
    }
    Ok(snapshot(&state))
}

#[tauri::command]
pub(crate) fn snooze_inbox_item(
    id: String,
    minutes: u32,
    app: tauri::AppHandle,
    state: tauri::State<AppState>,
) -> Result<Vec<InboxItem>, String> {
    let now = epoch_millis();
    let minutes = minutes.clamp(1, 1440);
    let until = now.saturating_add(u64::from(minutes) * 60_000);
    let changed = {
        let mut items = state.inbox.lock_recover();
        let changed = snooze_inbox(&mut items, &id, until);
        if changed {
            save_inbox(&state.inbox_path, &items)?;
        }
        changed
    };
    if changed {
        use tauri::Emitter;
        let _ = app.emit("inbox-changed", ());
    }
    Ok(snapshot(&state))
}

/// Push an item from a hook/command handler and notify the windows.
pub(crate) fn push_inbox(app: &tauri::AppHandle, state: &AppState, item: InboxItem) {
    let now = epoch_millis();
    {
        let mut items = state.inbox.lock_recover();
        upsert_inbox(&mut items, item, now);
        let _ = save_inbox(&state.inbox_path, &items);
    }
    use tauri::Emitter;
    let _ = app.emit("inbox-changed", ());
}

pub(crate) fn resolve_approval_inbox(app: &tauri::AppHandle, state: &AppState, approval_id: &str) {
    let now = epoch_millis();
    let changed = {
        let mut items = state.inbox.lock_recover();
        let changed = resolve_inbox_by_approval(&mut items, approval_id, now);
        if changed {
            let _ = save_inbox(&state.inbox_path, &items);
        }
        changed
    };
    if changed {
        use tauri::Emitter;
        let _ = app.emit("inbox-changed", ());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("agent-island-inbox-{tag}-{}", epoch_millis()))
            .join("inbox.json")
    }

    fn item(title: &str, dedupe: &str, at: u64) -> InboxItem {
        let mut item = InboxItem::new(
            "notification",
            "normal",
            "claude",
            "Claude Code",
            "session-1",
            title,
            "body",
            dedupe,
        );
        item.created_at_ms = at;
        item
    }

    #[test]
    fn round_trips_through_disk_and_survives_bom() {
        let path = temp_path("roundtrip");
        let items = vec![item("hello", "n-1", 10)];
        save_inbox(&path, &items).unwrap();
        assert_eq!(load_inbox(&path), items);

        // PowerShell's UTF8 encoding writes a BOM; it must not hide the inbox.
        let raw = fs::read_to_string(&path).unwrap();
        fs::write(&path, format!("\u{feff}{raw}")).unwrap();
        assert_eq!(load_inbox(&path).len(), 1);
        let _ = fs::remove_dir_all(path.parent().unwrap());
    }

    #[test]
    fn dedupe_refreshes_the_existing_item_instead_of_appending() {
        let mut items = vec![item("first", "same", 10)];
        let mut next = item("second", "same", 20);
        next.severity = "high".to_string();
        upsert_inbox(&mut items, next, 21);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "second");
        assert_eq!(items[0].severity, "high");
        assert_eq!(items[0].created_at_ms, 20);
    }

    #[test]
    fn resolve_and_snooze_change_state_without_dropping_the_item() {
        let mut items = vec![item("one", "one", 10), item("two", "two", 20)];
        let first = items[0].id.clone();
        let second = items[1].id.clone();
        assert!(resolve_inbox(&mut items, &first, 30));
        assert_eq!(items[0].state, "resolved");
        assert_eq!(items[0].resolved_at_ms, Some(30));
        assert!(snooze_inbox(&mut items, &second, 60));
        assert_eq!(items[1].state, "snoozed");
        assert_eq!(items[1].snooze_until_ms, Some(60));
        // A resolved item must not be snoozable.
        assert!(!snooze_inbox(&mut items, &first, 70));
    }

    #[test]
    fn expired_snooze_becomes_open_again() {
        let mut items = vec![item("one", "one", 10)];
        let id = items[0].id.clone();
        assert!(snooze_inbox(&mut items, &id, 50));
        normalize_snoozed(&mut items, 51);
        assert_eq!(items[0].state, "open");
        assert_eq!(items[0].snooze_until_ms, None);
    }

    #[test]
    fn caps_the_number_of_items_and_keeps_the_newest() {
        let mut items = Vec::new();
        for index in 0..(MAX_INBOX_ITEMS + 20) {
            items.push(item(
                &format!("n-{index}"),
                &format!("k-{index}"),
                index as u64,
            ));
        }
        prune_inbox(&mut items, MAX_INBOX_ITEMS as u64 + 100);
        assert_eq!(items.len(), MAX_INBOX_ITEMS);
        assert_eq!(items.first().unwrap().created_at_ms, 20);
        assert_eq!(
            items.last().unwrap().created_at_ms,
            (MAX_INBOX_ITEMS + 19) as u64
        );
    }

    #[test]
    fn drops_old_resolved_items_but_keeps_open_ones() {
        let mut items = vec![item("old-resolved", "a", 10), item("old-open", "b", 10)];
        items[0].state = "resolved".to_string();
        items[0].resolved_at_ms = Some(10);
        prune_inbox(&mut items, RESOLVED_RETENTION_MS + 20);
        assert_eq!(items.len(), 1);
        assert_eq!(items[0].title, "old-open");
    }

    #[test]
    fn sorts_unresolved_first_then_newest() {
        let mut items = vec![
            item("old", "a", 10),
            item("new", "b", 30),
            item("done", "c", 20),
        ];
        items[2].state = "resolved".to_string();
        sort_inbox(&mut items);
        assert_eq!(items[0].title, "new");
        assert_eq!(items[1].title, "old");
        assert_eq!(items[2].title, "done");
    }
}
