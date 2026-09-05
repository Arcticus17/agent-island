use serde::Serialize;
use sha2::{Digest, Sha256};

use crate::domain::{DataIssue, Freshness};

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct DiagnosticView {
    pub adapter: String,
    pub code: String,
    pub freshness: Freshness,
    pub event_type: Option<String>,
    pub event_count: usize,
    pub message_count: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skipped_lines: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub candidate_count: Option<usize>,
}

impl DiagnosticView {
    pub fn new(
        adapter: impl Into<String>,
        issue: DataIssue,
        freshness: Freshness,
        event_type: Option<String>,
        event_count: usize,
        message_count: usize,
    ) -> Self {
        Self {
            adapter: adapter.into(),
            code: issue.code().into(),
            freshness,
            event_type,
            event_count,
            message_count,
            skipped_lines: issue.skipped_lines(),
            candidate_count: issue.candidate_count(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct DiagnosticRecord {
    pub view: DiagnosticView,
    pub session_id: Option<String>,
    pub project_path: Option<String>,
    pub message_length: usize,
}

#[derive(Debug, Clone, Default)]
pub struct DiagnosticSnapshot {
    pub generated_at_ms: u64,
    pub records: Vec<DiagnosticRecord>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiagnosticsResponse {
    pub generated_at_ms: u64,
    pub issues: Vec<DiagnosticView>,
}

impl From<&DiagnosticSnapshot> for DiagnosticsResponse {
    fn from(snapshot: &DiagnosticSnapshot) -> Self {
        Self {
            generated_at_ms: snapshot.generated_at_ms,
            issues: snapshot
                .records
                .iter()
                .map(|record| record.view.clone())
                .collect(),
        }
    }
}

#[derive(Debug, Serialize)]
pub struct DiagnosticExport {
    pub app_version: String,
    pub os_version: String,
    pub generated_at_ms: u64,
    pub entries: Vec<DiagnosticExportEntry>,
}

#[derive(Debug, Serialize)]
pub struct DiagnosticExportEntry {
    pub adapter: String,
    pub session_hash: Option<String>,
    pub event_type: Option<String>,
    pub issue_code: String,
    pub event_count: usize,
    pub message_count: usize,
    pub skipped_lines: Option<usize>,
    pub candidate_count: Option<usize>,
    pub observed_at_ms: u64,
    pub stale: bool,
    pub project_basename: Option<String>,
    pub message_length: usize,
}

pub fn session_hash(session_id: &str) -> String {
    format!("{:x}", Sha256::digest(session_id.as_bytes()))
}

fn project_basename(path: &str) -> Option<String> {
    path.trim_end_matches(['/', '\\'])
        .split(['/', '\\'])
        .filter(|part| !part.is_empty())
        .next_back()
        .map(str::to_owned)
}

pub fn build_export_report(
    snapshot: &DiagnosticSnapshot,
    app_version: &str,
    os_version: &str,
) -> DiagnosticExport {
    DiagnosticExport {
        app_version: app_version.into(),
        os_version: os_version.into(),
        generated_at_ms: snapshot.generated_at_ms,
        entries: snapshot
            .records
            .iter()
            .map(|record| DiagnosticExportEntry {
                adapter: record.view.adapter.clone(),
                session_hash: record.session_id.as_deref().map(session_hash),
                event_type: record.view.event_type.clone(),
                issue_code: record.view.code.clone(),
                event_count: record.view.event_count,
                message_count: record.view.message_count,
                skipped_lines: record.view.skipped_lines,
                candidate_count: record.view.candidate_count,
                observed_at_ms: record.view.freshness.observed_at_ms,
                stale: record.view.freshness.stale,
                project_basename: record.project_path.as_deref().and_then(project_basename),
                message_length: record.message_length,
            })
            .collect(),
    }
}

pub fn write_export_report(
    destination: &std::path::Path,
    snapshot: &DiagnosticSnapshot,
    app_version: &str,
    os_version: &str,
) -> Result<(), String> {
    let report = build_export_report(snapshot, app_version, os_version);
    let json = serde_json::to_string_pretty(&report).map_err(|error| error.to_string())?;
    std::fs::write(destination, json).map_err(|error| error.to_string())
}

#[cfg(test)]
mod tests {
    use super::{build_export_report, DiagnosticRecord, DiagnosticSnapshot, DiagnosticView};
    use crate::domain::{DataIssue, Freshness};

    #[test]
    fn export_report_hashes_session_and_whitelists_private_metadata() {
        let snapshot = DiagnosticSnapshot {
            generated_at_ms: 77,
            records: vec![DiagnosticRecord {
                view: DiagnosticView::new(
                    "codex",
                    DataIssue::ParseFailed { skipped_lines: 2 },
                    Freshness {
                        observed_at_ms: 55,
                        stale: true,
                    },
                    Some("turn_failed".into()),
                    3,
                    4,
                ),
                session_id: Some("session-secret".into()),
                project_path: Some(r"D:\clients\secret-project".into()),
                message_length: 901,
            }],
        };

        let json =
            serde_json::to_value(build_export_report(&snapshot, "1.8.2", "Windows 11")).unwrap();
        let text = serde_json::to_string(&json).unwrap();

        assert_eq!(json["app_version"], "1.8.2");
        assert_eq!(json["os_version"], "Windows 11");
        assert_eq!(json["entries"][0]["adapter"], "codex");
        assert_eq!(json["entries"][0]["issue_code"], "parse_failed");
        assert_eq!(json["entries"][0]["skipped_lines"], 2);
        assert_eq!(json["entries"][0]["project_basename"], "secret-project");
        assert_eq!(json["entries"][0]["message_length"], 901);
        assert_eq!(
            json["entries"][0]["session_hash"].as_str().unwrap().len(),
            64
        );
        assert!(!text.contains("session-secret"));
        assert!(!text.contains(r"D:\clients"));
        assert!(!text.contains("prompt"));
        assert!(!text.contains("credential"));
        assert!(!text.contains("recent_output"));
    }

    #[test]
    fn session_hash_is_stable_sha256_and_not_adapter_dependent() {
        let first = super::session_hash("same-session");
        let second = super::session_hash("same-session");

        assert_eq!(first, second);
        assert_eq!(first.len(), 64);
        assert_ne!(first, "same-session");
    }
}
