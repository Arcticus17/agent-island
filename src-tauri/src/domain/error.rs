use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum DataIssue {
    LogUnavailable,
    ParseFailed { skipped_lines: usize },
    SessionAmbiguous { candidate_count: usize },
    ProcessGone,
    PermissionDenied,
}

impl DataIssue {
    pub fn code(&self) -> &'static str {
        match self {
            Self::LogUnavailable => "log_unavailable",
            Self::ParseFailed { .. } => "parse_failed",
            Self::SessionAmbiguous { .. } => "session_ambiguous",
            Self::ProcessGone => "process_gone",
            Self::PermissionDenied => "permission_denied",
        }
    }

    pub fn skipped_lines(&self) -> Option<usize> {
        match self {
            Self::ParseFailed { skipped_lines } => Some(*skipped_lines),
            _ => None,
        }
    }

    pub fn candidate_count(&self) -> Option<usize> {
        match self {
            Self::SessionAmbiguous { candidate_count } => Some(*candidate_count),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Freshness {
    pub observed_at_ms: u64,
    pub stale: bool,
}

pub fn adapter_freshness_window_ms(adapter: &str) -> u64 {
    match adapter {
        "hermes" => 120_000,
        "claude" | "codex" | "opencode" => 30_000,
        _ => 30_000,
    }
}

pub fn freshness_for_adapter(adapter: &str, observed_at_ms: u64, now_ms: u64) -> Freshness {
    Freshness {
        observed_at_ms,
        stale: now_ms.saturating_sub(observed_at_ms) > adapter_freshness_window_ms(adapter),
    }
}

pub fn parse_issue(valid_event_count: usize, skipped_lines: usize) -> Option<DataIssue> {
    (valid_event_count == 0 && skipped_lines > 0)
        .then_some(DataIssue::ParseFailed { skipped_lines })
}
