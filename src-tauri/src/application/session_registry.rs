use crate::domain::{ProcessIdentity, SessionIdentity, EVENT_CLOCK_SKEW_TOLERANCE_MS};

const PROCESS_START_GRACE_MS: u64 = 5_000;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMatch {
    Confirmed(String),
    Probable(String),
    Ambiguous(Vec<String>),
    Unknown,
}

#[derive(Debug)]
struct SessionEvidence {
    session_id: String,
    process_ids: Vec<u32>,
    has_matching_project: bool,
    has_missing_project: bool,
    has_nonmatching_project: bool,
}

impl SessionEvidence {
    fn has_project_conflict(&self) -> bool {
        self.has_matching_project && self.has_nonmatching_project
    }
}

pub fn match_active_session(
    process: &ProcessIdentity,
    candidates: &[SessionIdentity],
    now_ms: u64,
) -> SessionMatch {
    let earliest_event_at_ms = process.started_at_ms.saturating_sub(PROCESS_START_GRACE_MS);
    let latest_event_at_ms = now_ms.saturating_add(EVENT_CLOCK_SKEW_TOLERANCE_MS);
    let process_path = present_project_path(process.project_path.as_deref());
    let mut identities: Vec<SessionEvidence> = Vec::new();

    for candidate in candidates.iter().filter(|candidate| {
        candidate.agent_id == process.agent_id
            && candidate.last_event_at_ms >= earliest_event_at_ms
            && candidate.last_event_at_ms <= latest_event_at_ms
    }) {
        let candidate_path = present_project_path(candidate.project_path.as_deref());
        let (has_matching_project, has_nonmatching_project) = match (process_path, candidate_path) {
            (Some(process_path), Some(candidate_path))
                if projects_match(process_path, candidate_path) =>
            {
                (true, false)
            }
            (Some(_), Some(_)) => (false, true),
            _ => (false, false),
        };
        let has_missing_project = candidate_path.is_none();

        if let Some(identity) = identities
            .iter_mut()
            .find(|identity| identity.session_id == candidate.session_id)
        {
            identity
                .process_ids
                .extend_from_slice(&candidate.process_ids);
            identity.has_matching_project |= has_matching_project;
            identity.has_missing_project |= has_missing_project;
            identity.has_nonmatching_project |= has_nonmatching_project;
        } else {
            identities.push(SessionEvidence {
                session_id: candidate.session_id.clone(),
                process_ids: candidate.process_ids.clone(),
                has_matching_project,
                has_missing_project,
                has_nonmatching_project,
            });
        }
    }

    let confirmed: Vec<&SessionEvidence> = identities
        .iter()
        .filter(|identity| shares_process_id(process, &identity.process_ids))
        .collect();
    match confirmed.as_slice() {
        [] => {}
        [identity] if !identity.has_project_conflict() => {
            return SessionMatch::Confirmed(identity.session_id.clone())
        }
        identities => return SessionMatch::Ambiguous(session_ids(identities.iter().copied())),
    }

    if process_path.is_none() {
        return match identities.as_slice() {
            [] => SessionMatch::Unknown,
            identities => SessionMatch::Ambiguous(session_ids(identities.iter())),
        };
    }

    let probable: Vec<&SessionEvidence> = identities
        .iter()
        .filter(|identity| {
            identity.has_matching_project
                || identity.has_missing_project
                || identity.has_project_conflict()
        })
        .collect();
    match probable.as_slice() {
        [] => SessionMatch::Unknown,
        [identity]
            if identity.has_matching_project
                && !identity.has_missing_project
                && !identity.has_project_conflict() =>
        {
            SessionMatch::Probable(identity.session_id.clone())
        }
        identities => SessionMatch::Ambiguous(session_ids(identities.iter().copied())),
    }
}

fn session_ids<'a>(identities: impl Iterator<Item = &'a SessionEvidence>) -> Vec<String> {
    identities
        .map(|identity| identity.session_id.clone())
        .collect()
}

fn present_project_path(path: Option<&str>) -> Option<&str> {
    path.filter(|path| !path.trim().is_empty())
}

fn projects_match(process_path: &str, candidate_path: &str) -> bool {
    normalize_windows_path(process_path) == normalize_windows_path(candidate_path)
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

fn shares_process_id(process: &ProcessIdentity, candidate_process_ids: &[u32]) -> bool {
    candidate_process_ids
        .iter()
        .any(|candidate_id| process.process_ids.contains(candidate_id))
}

#[cfg(test)]
mod tests {
    use super::{match_active_session as match_active_session_at, SessionMatch};
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

    fn match_active_session(
        process: &ProcessIdentity,
        candidates: &[SessionIdentity],
    ) -> SessionMatch {
        match_active_session_at(process, candidates, 30_000)
    }

    fn match_at(
        process: &ProcessIdentity,
        candidates: &[SessionIdentity],
        now_ms: u64,
    ) -> SessionMatch {
        match_active_session_at(process, candidates, now_ms)
    }

    #[test]
    fn rejects_candidate_events_beyond_the_clock_skew_allowance() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let tolerated = session("codex", "tolerated", Some(r"D:\work\app"), vec![], 35_000);
        let future = session("codex", "future", Some(r"D:\work\app"), vec![], 35_001);

        assert_eq!(
            match_at(&process, &[tolerated], 30_000),
            SessionMatch::Probable("tolerated".into())
        );
        assert_eq!(match_at(&process, &[future], 30_000), SessionMatch::Unknown);
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
    fn confirms_process_id_evidence_over_an_unlinked_probable_candidate() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let linked = session("codex", "linked", Some(r"D:\work\app"), vec![10], 21_000);
        let unlinked = session("codex", "unlinked", Some(r"D:\work\app"), vec![], 21_000);

        assert_eq!(
            match_active_session(&process, &[linked, unlinked]),
            SessionMatch::Confirmed("linked".into())
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
    fn rejects_candidates_with_a_different_agent() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let other_agent = session(
            "claude",
            "other-agent",
            Some(r"D:\work\app"),
            vec![],
            21_000,
        );

        assert_eq!(
            match_active_session(&process, &[other_agent]),
            SessionMatch::Unknown
        );
    }

    #[test]
    fn merges_process_id_evidence_across_duplicate_session_records() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let path_record = session("codex", "same", Some(r"D:\work\app"), vec![], 21_000);
        let pid_record = session("codex", "same", Some(r"d:/WORK/app/"), vec![10], 21_000);

        assert_eq!(
            match_active_session(&process, &[path_record, pid_record]),
            SessionMatch::Confirmed("same".into())
        );
    }

    #[test]
    fn keeps_multiple_confirmed_session_ids_ambiguous() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let first = session("codex", "first", Some(r"D:\one"), vec![10], 21_000);
        let second = session("codex", "second", Some(r"D:\two"), vec![10], 21_000);

        assert_eq!(
            match_active_session(&process, &[first, second]),
            SessionMatch::Ambiguous(vec!["first".into(), "second".into()])
        );
    }

    #[test]
    fn treats_a_candidate_without_a_project_path_as_ambiguous() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let missing = session("codex", "missing", None, vec![], 21_000);
        let matching = session("codex", "matching", Some(r"D:\work\app"), vec![], 21_000);

        assert_eq!(
            match_active_session(&process, &[missing, matching]),
            SessionMatch::Ambiguous(vec!["missing".into(), "matching".into()])
        );
    }

    #[test]
    fn treats_a_missing_process_project_path_as_ambiguous() {
        let process = process(None, vec![10], 20_000);
        let candidate = session("codex", "candidate", Some(r"D:\work\app"), vec![], 21_000);

        assert_eq!(
            match_active_session(&process, &[candidate]),
            SessionMatch::Ambiguous(vec!["candidate".into()])
        );
    }

    #[test]
    fn treats_an_empty_or_whitespace_project_path_as_missing() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let empty = session("codex", "empty", Some(""), vec![], 21_000);
        let whitespace = session("codex", "whitespace", Some(" \t"), vec![], 21_000);

        assert_eq!(
            match_active_session(&process, &[empty, whitespace]),
            SessionMatch::Ambiguous(vec!["empty".into(), "whitespace".into()])
        );
    }

    #[test]
    fn treats_a_whitespace_process_project_path_as_missing() {
        let process = process(Some(" \t"), vec![10], 20_000);
        let candidate = session("codex", "candidate", Some(r"D:\work\app"), vec![], 21_000);

        assert_eq!(
            match_active_session(&process, &[candidate]),
            SessionMatch::Ambiguous(vec!["candidate".into()])
        );
    }

    #[test]
    fn rejects_nonempty_different_project_paths() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let candidate = session("codex", "other", Some(r"D:\work\other"), vec![], 21_000);

        assert_eq!(
            match_active_session(&process, &[candidate]),
            SessionMatch::Unknown
        );
    }

    #[test]
    fn normalizes_unc_share_roots_with_mixed_separators() {
        let process = process(Some(r"\\server\share\"), vec![10], 20_000);
        let candidate = session("codex", "share", Some("//SERVER/share//"), vec![], 21_000);

        assert_eq!(
            match_active_session(&process, &[candidate]),
            SessionMatch::Probable("share".into())
        );
    }

    #[test]
    fn deduplicates_repeated_session_ids_for_probable_matches() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let first = session("codex", "same", Some(r"D:\work\app"), vec![], 21_000);
        let duplicate = session("codex", "same", Some(r"d:/WORK/app/"), vec![], 22_000);

        assert_eq!(
            match_active_session(&process, &[first, duplicate]),
            SessionMatch::Probable("same".into())
        );
    }

    #[test]
    fn keeps_duplicate_session_records_with_conflicting_project_paths_ambiguous() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let matching = session("codex", "same", Some(r"D:\work\app"), vec![], 21_000);
        let conflicting = session("codex", "same", Some(r"D:\other"), vec![], 21_000);

        assert_eq!(
            match_active_session(&process, &[matching, conflicting]),
            SessionMatch::Ambiguous(vec!["same".into()])
        );
    }

    #[test]
    fn does_not_confirm_process_id_evidence_for_conflicting_duplicate_project_paths() {
        let process = process(Some(r"D:\work\app"), vec![10], 20_000);
        let matching = session("codex", "same", Some(r"D:\work\app"), vec![], 21_000);
        let conflicting = session("codex", "same", Some(r"D:\other"), vec![10], 21_000);

        assert_eq!(
            match_active_session(&process, &[matching, conflicting]),
            SessionMatch::Ambiguous(vec!["same".into()])
        );
    }
}
