use crate::domain::{ProcessIdentity, SessionIdentity};

const PROCESS_START_GRACE_MS: u64 = 5_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMatch {
    Confirmed(String),
    Probable(String),
    Ambiguous(Vec<String>),
    Unknown,
}

pub fn match_active_session(
    process: &ProcessIdentity,
    candidates: &[SessionIdentity],
) -> SessionMatch {
    let earliest_event_at_ms = process.started_at_ms.saturating_sub(PROCESS_START_GRACE_MS);
    let matching: Vec<&SessionIdentity> = candidates
        .iter()
        .filter(|candidate| {
            candidate.agent_id == process.agent_id
                && projects_match(
                    process.project_path.as_deref(),
                    candidate.project_path.as_deref(),
                )
                && candidate.last_event_at_ms >= earliest_event_at_ms
        })
        .collect();

    match matching.as_slice() {
        [] => SessionMatch::Unknown,
        [candidate] if shares_process_id(process, candidate) => {
            SessionMatch::Confirmed(candidate.session_id.clone())
        }
        [candidate] => SessionMatch::Probable(candidate.session_id.clone()),
        candidates => SessionMatch::Ambiguous(
            candidates
                .iter()
                .map(|candidate| candidate.session_id.clone())
                .collect(),
        ),
    }
}

fn projects_match(process_path: Option<&str>, candidate_path: Option<&str>) -> bool {
    match (process_path, candidate_path) {
        (Some(process_path), Some(candidate_path)) => {
            normalize_windows_path(process_path) == normalize_windows_path(candidate_path)
        }
        _ => false,
    }
}

fn normalize_windows_path(path: &str) -> String {
    let mut normalized = path.replace('/', "\\").to_lowercase();
    while normalized.ends_with('\\') && !is_drive_root(&normalized) && normalized != "\\" {
        normalized.pop();
    }
    normalized
}

fn is_drive_root(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() == 3 && bytes[1] == b':' && bytes[2] == b'\\'
}

fn shares_process_id(process: &ProcessIdentity, candidate: &SessionIdentity) -> bool {
    candidate
        .process_ids
        .iter()
        .any(|candidate_id| process.process_ids.contains(candidate_id))
}

#[cfg(test)]
mod tests {
    use super::{match_active_session, SessionMatch};
    use crate::domain::{
        Confidence, EventSource, ProcessIdentity, SessionIdentity, SessionLifecycle,
    };

    fn process(
        project_path: Option<&str>,
        process_ids: Vec<u32>,
        started_at_ms: u64,
    ) -> ProcessIdentity {
        ProcessIdentity {
            agent_id: "codex".into(),
            project_path: project_path.map(str::to_owned),
            process_ids,
            started_at_ms,
        }
    }

    fn session(
        agent_id: &str,
        session_id: &str,
        project_path: Option<&str>,
        process_ids: Vec<u32>,
        last_event_at_ms: u64,
    ) -> SessionIdentity {
        SessionIdentity {
            agent_id: agent_id.into(),
            session_id: session_id.into(),
            project_path: project_path.map(str::to_owned),
            process_ids,
            started_at_ms: 19_000,
            last_event_at_ms,
            source: EventSource::CodexLog,
            confidence: Confidence::Unknown,
            lifecycle: SessionLifecycle::Historical,
        }
    }

    #[test]
    fn rejects_history_created_before_process_start_window() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let old = session("codex", "old", Some(r"D:\work\app"), vec![], 14_999);

        assert_eq!(
            match_active_session(&process, &[old]),
            SessionMatch::Unknown
        );
    }

    #[test]
    fn accepts_an_event_exactly_at_the_process_start_window_boundary() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let boundary = session("codex", "boundary", Some(r"D:\work\app"), vec![], 15_000);

        assert_eq!(
            match_active_session(&process, &[boundary]),
            SessionMatch::Probable("boundary".into())
        );
    }

    #[test]
    fn accepts_unique_same_project_candidate_with_case_and_trailing_separator_differences() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let fresh = session("codex", "fresh", Some(r"d:/WORK/app/"), vec![], 21_000);

        assert_eq!(
            match_active_session(&process, &[fresh]),
            SessionMatch::Probable("fresh".into())
        );
    }

    #[test]
    fn accepts_windows_drive_root_with_a_trailing_separator() {
        let process = process(Some(r"D:\"), vec![10], 20_000);
        let fresh = session("codex", "root", Some("d:/"), vec![], 20_000);

        assert_eq!(
            match_active_session(&process, &[fresh]),
            SessionMatch::Probable("root".into())
        );
    }

    #[test]
    fn preserves_source_order_when_two_candidates_are_equally_valid() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let one = session("codex", "one", Some(r"D:\work\app"), vec![], 21_000);
        let two = session("codex", "two", Some(r"D:\work\app"), vec![], 25_000);

        assert_eq!(
            match_active_session(&process, &[one, two]),
            SessionMatch::Ambiguous(vec!["one".into(), "two".into()])
        );
    }

    #[test]
    fn confirms_a_unique_candidate_with_a_shared_process_id() {
        let process = process(Some(r"D:\work\app"), vec![10, 11], 20_000);
        let fresh = session("codex", "fresh", Some(r"D:\work\app"), vec![11], 21_000);

        assert_eq!(
            match_active_session(&process, &[fresh]),
            SessionMatch::Confirmed("fresh".into())
        );
    }

    #[test]
    fn keeps_ambiguity_when_only_one_of_multiple_candidates_shares_a_process_id() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let linked = session("codex", "linked", Some(r"D:\work\app"), vec![10], 21_000);
        let unlinked = session("codex", "unlinked", Some(r"D:\work\app"), vec![], 21_000);

        assert_eq!(
            match_active_session(&process, &[linked, unlinked]),
            SessionMatch::Ambiguous(vec!["linked".into(), "unlinked".into()])
        );
    }

    #[test]
    fn accepts_an_event_at_zero_when_process_start_is_before_safety_window() {
        let process = process(Some(r"D:\work\app"), vec![10], 4_999);
        let fresh = session("codex", "fresh", Some(r"D:\work\app"), vec![], 0);

        assert_eq!(
            match_active_session(&process, &[fresh]),
            SessionMatch::Probable("fresh".into())
        );
    }

    #[test]
    fn rejects_candidates_with_a_different_agent_or_missing_project_path() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let other_agent = session(
            "claude",
            "other-agent",
            Some(r"D:\work\app"),
            vec![],
            21_000,
        );
        let missing_project = session("codex", "missing-project", None, vec![], 21_000);

        assert_eq!(
            match_active_session(&process, &[other_agent, missing_project]),
            SessionMatch::Unknown
        );
    }
}
