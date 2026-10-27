//! Agent 自报身份（hooks 通道）。
//!
//! Claude Code 的 hook 会带着 `session_id` 和 `cwd` 主动告诉我们"我是谁、在哪个目录"。
//! 这是比"读日志猜"更权威的信号：只要最近有 hook 上报过，这个 Agent 的会话身份就是确定的。
//!
//! 记录有有效期（`HOOKED_TTL_MS`）：session 结束后不该继续声称"已确认"。

use crate::*;
use std::collections::HashMap;

/// 自报身份的有效期：Claude 一次会话可能长时间空闲，10 分钟内仍视为可信。
pub(crate) const HOOKED_TTL_MS: u64 = 10 * 60 * 1000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HookedSession {
    pub session_id: String,
    pub cwd: Option<String>,
    pub at_ms: u64,
}

/// 记录一次自报。空 session（hook 没带 id）不覆盖已有记录。
pub(crate) fn record_hooked(
    hooked: &mut HashMap<String, HookedSession>,
    agent_id: &str,
    session_id: &str,
    cwd: Option<&str>,
    now_ms: u64,
) -> bool {
    let session_id = session_id.trim();
    if session_id.is_empty() {
        return false;
    }
    hooked.insert(
        agent_id.to_string(),
        HookedSession {
            session_id: session_id.to_string(),
            cwd: cwd.map(str::trim).filter(|cwd| !cwd.is_empty()).map(str::to_owned),
            at_ms: now_ms,
        },
    );
    true
}

/// 取仍然有效的自报身份。
pub(crate) fn fresh_hooked<'a>(
    hooked: &'a HashMap<String, HookedSession>,
    agent_id: &str,
    now_ms: u64,
) -> Option<&'a HookedSession> {
    hooked.get(agent_id).filter(|record| {
        record.at_ms <= now_ms.saturating_add(domain::EVENT_CLOCK_SKEW_TOLERANCE_MS)
            && now_ms.saturating_sub(record.at_ms) <= HOOKED_TTL_MS
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn records_and_expires_self_reported_identity() {
        let now = 1_000_000;
        let mut hooked = HashMap::new();
        assert!(record_hooked(&mut hooked, "claude", "s-1", Some("D:/work"), now));
        let record = fresh_hooked(&hooked, "claude", now + 60_000).expect("still fresh");
        assert_eq!(record.session_id, "s-1");
        assert_eq!(record.cwd.as_deref(), Some("D:/work"));
        // 超过有效期就不可信了
        assert!(fresh_hooked(&hooked, "claude", now + HOOKED_TTL_MS + 1).is_none());
        // 未知 Agent 不受影响
        assert!(fresh_hooked(&hooked, "codex", now).is_none());
    }

    #[test]
    fn empty_session_does_not_wipe_a_good_record() {
        let mut hooked = HashMap::new();
        record_hooked(&mut hooked, "claude", "s-1", None, 10);
        assert!(!record_hooked(&mut hooked, "claude", "   ", None, 20));
        assert_eq!(hooked["claude"].session_id, "s-1");
        // 只带 session 不带 cwd 也允许
        assert!(record_hooked(&mut hooked, "claude", "s-2", Some("  "), 30));
        assert_eq!(hooked["claude"].cwd, None);
        assert_eq!(hooked["claude"].session_id, "s-2");
    }

    #[test]
    fn future_timestamps_within_skew_are_still_trusted() {
        let now = 5_000;
        let mut hooked = HashMap::new();
        record_hooked(&mut hooked, "claude", "s-1", None, now + 1_000);
        assert!(fresh_hooked(&hooked, "claude", now).is_some());
        record_hooked(&mut hooked, "claude", "s-2", None, now + 60_000);
        assert!(fresh_hooked(&hooked, "claude", now).is_none());
    }
}
