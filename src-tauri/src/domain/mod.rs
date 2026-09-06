pub mod error;
pub mod event;
pub mod session;
pub mod status;

pub use error::*;
pub use event::*;
pub use session::*;
pub use status::*;

#[cfg(test)]
mod error_contract_tests {
    use super::{freshness_for_adapter, parse_issue, DataIssue};
    use crate::adapters::{claude::ClaudeAdapter, AgentAdapter};
    use serde_json::json;

    #[test]
    fn diagnostic_codes_are_stable_snake_case_and_keep_counts() {
        assert_eq!(
            serde_json::to_value(DataIssue::LogUnavailable).unwrap(),
            json!({ "code": "log_unavailable" })
        );
        assert_eq!(
            serde_json::to_value(DataIssue::ParseFailed { skipped_lines: 7 }).unwrap(),
            json!({ "code": "parse_failed", "skipped_lines": 7 })
        );
        assert_eq!(
            serde_json::to_value(DataIssue::SessionAmbiguous { candidate_count: 3 }).unwrap(),
            json!({ "code": "session_ambiguous", "candidate_count": 3 })
        );
        assert_eq!(
            serde_json::to_value(DataIssue::ProcessGone).unwrap(),
            json!({ "code": "process_gone" })
        );
        assert_eq!(
            serde_json::to_value(DataIssue::PermissionDenied).unwrap(),
            json!({ "code": "permission_denied" })
        );
    }

    #[test]
    fn malformed_line_is_isolated_when_structured_events_remain() {
        let text = concat!(
            "{\"sessionId\":\"session-a\",\"uuid\":\"event-a\",\"timestamp\":\"2026-09-04T10:00:00Z\",\"type\":\"user\",\"message\":{\"role\":\"user\",\"content\":[{\"type\":\"text\",\"text\":\"hello\"}]}}\n",
            "not-json\n",
        );

        let report = ClaudeAdapter.parse(text);

        assert_eq!(report.events.len(), 1);
        assert_eq!(report.skipped_lines, 1);
        assert_eq!(
            parse_issue(
                !report.events.is_empty()
                    || !report.messages.is_empty()
                    || !report.sessions.is_empty(),
                report.skipped_lines,
            ),
            Some(DataIssue::PartialParse { skipped_lines: 1 })
        );
    }

    #[test]
    fn report_without_valid_structured_events_is_parse_failed() {
        let report = ClaudeAdapter.parse("not-json\n{\"type\":\"unknown\"}\n");

        assert!(report.events.is_empty());
        assert_eq!(
            parse_issue(
                !report.events.is_empty()
                    || !report.messages.is_empty()
                    || !report.sessions.is_empty(),
                report.skipped_lines,
            ),
            Some(DataIssue::ParseFailed {
                skipped_lines: report.skipped_lines,
            })
        );
    }

    #[test]
    fn adapter_freshness_windows_keep_hermes_longer_than_log_adapters() {
        assert!(freshness_for_adapter("codex", 20_000, 50_001).stale);
        assert!(!freshness_for_adapter("hermes", 20_000, 50_001).stale);
        assert!(freshness_for_adapter("hermes", 20_000, 140_001).stale);
    }

    #[test]
    fn adapter_freshness_allows_small_clock_skew_but_rejects_distant_future_events() {
        assert!(!freshness_for_adapter("codex", 25_000, 20_000).stale);
        assert!(freshness_for_adapter("codex", 25_001, 20_000).stale);
    }
}
