use serde::Serialize;

use crate::application::session_registry::{match_active_session, SessionMatch};
use crate::domain::{
    derive_display_status, freshness_for_adapter, AgentState, AttentionState, DataIssue,
    DisplayStatus, DomainEvent, EventKind, Freshness, ProcessIdentity, ProcessState,
    SessionIdentity, TurnState,
};
use crate::interface::diagnostics::DiagnosticView;

#[derive(Debug, Clone)]
pub struct ProcessFact {
    pub name: String,
    pub identity: ProcessIdentity,
    pub process_state: ProcessState,
}

#[derive(Debug, Clone)]
pub struct SessionCandidate {
    pub identity: SessionIdentity,
    pub view: SessionView,
    pub events: Vec<DomainEvent>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentViewSnapshot {
    pub schema_version: u16,
    pub generated_at_ms: u64,
    pub agents: Vec<AgentView>,
}

#[derive(Debug, Clone, Serialize)]
pub struct AgentView {
    pub id: String,
    pub name: String,
    pub state: AgentState,
    pub display_status: DisplayStatus,
    pub active_session: Option<SessionView>,
    pub history_sessions: Vec<SessionSummary>,
    pub diagnostic: Option<DiagnosticView>,
    pub freshness: Freshness,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub id: String,
    pub name: String,
    pub cwd: Option<String>,
    pub log_path: Option<String>,
    pub recent_output: Vec<String>,
    pub current_file: Option<String>,
    pub log_status: Option<String>,
    pub alert: Option<String>,
    pub display_status: DisplayStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub cwd: Option<String>,
    pub display_status: DisplayStatus,
}

impl From<&SessionView> for SessionSummary {
    fn from(session: &SessionView) -> Self {
        Self {
            id: session.id.clone(),
            name: session.name.clone(),
            cwd: session.cwd.clone(),
            display_status: session.display_status,
        }
    }
}

pub fn build_snapshot(
    now_ms: u64,
    process_facts: &[ProcessFact],
    session_candidates: &[SessionCandidate],
) -> AgentViewSnapshot {
    let agents = process_facts
        .iter()
        .map(|process| build_agent_view(now_ms, process, session_candidates))
        .collect();

    AgentViewSnapshot {
        schema_version: 1,
        generated_at_ms: now_ms,
        agents,
    }
}

fn build_agent_view(
    now_ms: u64,
    process: &ProcessFact,
    session_candidates: &[SessionCandidate],
) -> AgentView {
    let agent_candidates: Vec<&SessionCandidate> = session_candidates
        .iter()
        .filter(|candidate| candidate.identity.agent_id == process.identity.agent_id)
        .collect();
    let identities: Vec<SessionIdentity> = agent_candidates
        .iter()
        .map(|candidate| candidate.identity.clone())
        .collect();
    let session_match = if process.process_state == ProcessState::Running {
        match_active_session(&process.identity, &identities)
    } else {
        SessionMatch::Unknown
    };
    let (active_session_id, diagnostic_issue) = match session_match {
        SessionMatch::Confirmed(session_id) | SessionMatch::Probable(session_id) => {
            (Some(session_id), None)
        }
        SessionMatch::Ambiguous(candidate_ids) => (
            None,
            Some(DataIssue::SessionAmbiguous {
                candidate_count: candidate_ids.len(),
            }),
        ),
        SessionMatch::Unknown => (None, None),
    };
    let observed_at_ms = active_session_id
        .as_deref()
        .and_then(|session_id| {
            agent_candidates
                .iter()
                .find(|candidate| candidate.identity.session_id == session_id)
                .map(|candidate| candidate.identity.last_event_at_ms)
        })
        .unwrap_or(now_ms);
    let freshness = freshness_for_adapter(&process.identity.agent_id, observed_at_ms, now_ms);
    let diagnostic = diagnostic_issue
        .map(|issue| DiagnosticView::new(&process.identity.agent_id, issue, freshness, None, 0, 0));
    let active_session = active_session_id.as_deref().and_then(|session_id| {
        agent_candidates
            .iter()
            .find(|candidate| candidate.identity.session_id == session_id)
            .map(|candidate| {
                session_view(now_ms, candidate, process.process_state, &agent_candidates)
            })
    });
    let history_sessions = agent_candidates
        .iter()
        .filter(|candidate| {
            Some(candidate.identity.session_id.as_str()) != active_session_id.as_deref()
        })
        .map(|candidate| {
            let mut view = candidate.view.clone();
            view.display_status = DisplayStatus::Idle;
            SessionSummary::from(&view)
        })
        .collect();
    let state = reduce_agent_state(
        process.process_state,
        active_session_id.as_deref(),
        &agent_candidates,
    );

    AgentView {
        id: process.identity.agent_id.clone(),
        name: process.name.clone(),
        display_status: derive_display_status(&state, now_ms),
        state,
        active_session,
        history_sessions,
        diagnostic,
        freshness,
    }
}

fn session_view(
    now_ms: u64,
    candidate: &SessionCandidate,
    process_state: ProcessState,
    candidates: &[&SessionCandidate],
) -> SessionView {
    let state = reduce_agent_state(
        process_state,
        Some(&candidate.identity.session_id),
        candidates,
    );
    let mut view = candidate.view.clone();
    view.display_status = derive_display_status(&state, now_ms);
    view
}

fn reduce_agent_state(
    process_state: ProcessState,
    active_session_id: Option<&str>,
    candidates: &[&SessionCandidate],
) -> AgentState {
    let mut state = AgentState {
        process: process_state,
        turn: TurnState::Idle,
        attention: AttentionState::None,
        result_at_ms: None,
    };
    let Some(active_session_id) = active_session_id else {
        return state;
    };
    let mut events: Vec<&DomainEvent> = candidates
        .iter()
        .flat_map(|candidate| candidate.events.iter())
        .filter(|event| event.session_id == active_session_id)
        .collect();
    events.sort_by_key(|event| event.at_ms);

    for event in events {
        match event.kind {
            EventKind::TurnStarted | EventKind::ToolStarted | EventKind::ToolFinished { .. } => {
                state.turn = TurnState::Executing;
                state.attention = AttentionState::None;
                state.result_at_ms = None;
            }
            EventKind::AttentionRequested { approval } => {
                state.attention = if approval {
                    AttentionState::ApprovalRequired
                } else {
                    AttentionState::InputRequired
                };
            }
            EventKind::TurnSucceeded => {
                state.turn = TurnState::Succeeded;
                state.attention = AttentionState::None;
                state.result_at_ms = Some(event.at_ms);
            }
            EventKind::TurnFailed => {
                state.turn = TurnState::Failed;
                state.attention = AttentionState::None;
                state.result_at_ms = Some(event.at_ms);
            }
            EventKind::DiagnosticHint { .. } => {}
        }
    }
    state
}

#[cfg(test)]
mod tests {
    use super::{build_snapshot, ProcessFact, SessionCandidate, SessionView};
    use crate::domain::{
        AttentionState, Confidence, DisplayStatus, DomainEvent, EventKind, EventSource,
        ProcessIdentity, ProcessState, SessionIdentity, SessionLifecycle, TurnState,
    };

    fn session_candidate(
        session_id: &str,
        project_path: &str,
        last_event_at_ms: u64,
        lifecycle: SessionLifecycle,
        kind: EventKind,
    ) -> SessionCandidate {
        SessionCandidate {
            identity: SessionIdentity {
                agent_id: "codex".into(),
                session_id: session_id.into(),
                project_path: Some(project_path.into()),
                process_ids: Vec::new(),
                started_at_ms: last_event_at_ms.saturating_sub(1_000),
                last_event_at_ms,
                source: EventSource::CodexLog,
                confidence: Confidence::Probable,
                lifecycle,
            },
            view: SessionView {
                id: session_id.into(),
                name: session_id.into(),
                cwd: Some(project_path.into()),
                log_path: None,
                recent_output: Vec::new(),
                current_file: None,
                log_status: None,
                alert: None,
                display_status: DisplayStatus::Idle,
            },
            events: vec![DomainEvent {
                agent_id: "codex".into(),
                session_id: session_id.into(),
                turn_id: format!("{session_id}-turn"),
                at_ms: last_event_at_ms,
                source: EventSource::CodexLog,
                confidence: Confidence::Confirmed,
                kind,
            }],
        }
    }

    #[test]
    fn snapshot_uses_matching_session_not_newest_history() {
        let process = ProcessFact {
            name: "Codex CLI".into(),
            identity: ProcessIdentity {
                agent_id: "codex".into(),
                project_path: Some(r"D:\work\active".into()),
                process_ids: vec![42],
                started_at_ms: 20_000,
            },
            process_state: ProcessState::Running,
        };
        let matching = session_candidate(
            "matching-active",
            r"D:\work\active",
            21_000,
            SessionLifecycle::Active,
            EventKind::ToolStarted,
        );
        let unrelated = session_candidate(
            "newer-history",
            r"D:\work\other",
            30_000,
            SessionLifecycle::Historical,
            EventKind::TurnFailed,
        );

        let snapshot = build_snapshot(31_000, &[process], &[matching, unrelated]);

        let agent = &snapshot.agents[0];
        assert_eq!(snapshot.schema_version, 1);
        assert_eq!(snapshot.generated_at_ms, 31_000);
        let active_session = agent.active_session.as_ref().unwrap();
        assert_eq!(active_session.id, "matching-active");
        assert_eq!(active_session.display_status, DisplayStatus::Working);
        assert_eq!(
            agent
                .history_sessions
                .iter()
                .map(|session| session.id.as_str())
                .collect::<Vec<_>>(),
            vec!["newer-history"]
        );
        assert_eq!(agent.state.process, ProcessState::Running);
        assert_eq!(agent.state.turn, TurnState::Executing);
        assert_eq!(agent.state.attention, AttentionState::None);
        assert_eq!(agent.display_status, DisplayStatus::Working);
        assert_eq!(agent.freshness.observed_at_ms, 21_000);
        assert_eq!(
            agent.history_sessions[0].display_status,
            DisplayStatus::Idle
        );
    }

    #[test]
    fn ambiguous_sessions_have_no_active_session_and_emit_a_diagnostic() {
        let process = ProcessFact {
            name: "Codex CLI".into(),
            identity: ProcessIdentity {
                agent_id: "codex".into(),
                project_path: Some(r"D:\work\active".into()),
                process_ids: vec![42],
                started_at_ms: 20_000,
            },
            process_state: ProcessState::Running,
        };
        let first = session_candidate(
            "candidate-one",
            r"D:\work\active",
            21_000,
            SessionLifecycle::Active,
            EventKind::ToolStarted,
        );
        let second = session_candidate(
            "candidate-two",
            r"D:\work\active",
            22_000,
            SessionLifecycle::Active,
            EventKind::AttentionRequested { approval: true },
        );

        let snapshot = build_snapshot(23_000, &[process], &[first, second]);

        let agent = &snapshot.agents[0];
        assert!(agent.active_session.is_none());
        assert_eq!(agent.diagnostic.as_ref().unwrap().code, "session_ambiguous");
        assert_eq!(agent.diagnostic.as_ref().unwrap().candidate_count, Some(2));
        assert_eq!(agent.state.turn, TurnState::Idle);
        assert_eq!(agent.display_status, DisplayStatus::Idle);
        assert_eq!(agent.history_sessions.len(), 2);
        assert!(agent
            .history_sessions
            .iter()
            .all(|session| session.display_status == DisplayStatus::Idle));
        assert_eq!(agent.freshness.observed_at_ms, 23_000);
        assert_eq!(
            agent.diagnostic.as_ref().unwrap().freshness.observed_at_ms,
            23_000
        );
    }

    #[test]
    fn unknown_running_session_uses_process_observation_not_unrelated_history() {
        let process = ProcessFact {
            name: "Codex CLI".into(),
            identity: ProcessIdentity {
                agent_id: "codex".into(),
                project_path: Some(r"D:\work\active".into()),
                process_ids: vec![42],
                started_at_ms: 20_000,
            },
            process_state: ProcessState::Running,
        };
        let unrelated = session_candidate(
            "unrelated-history",
            r"D:\work\other",
            99_000,
            SessionLifecycle::Historical,
            EventKind::TurnFailed,
        );

        let snapshot = build_snapshot(100_000, &[process], &[unrelated]);

        let agent = &snapshot.agents[0];
        assert!(agent.active_session.is_none());
        assert_eq!(agent.freshness.observed_at_ms, 100_000);
        assert!(!agent.freshness.stale);
    }

    #[test]
    fn stopped_process_uses_current_observation_not_old_history() {
        let process = ProcessFact {
            name: "Codex CLI".into(),
            identity: ProcessIdentity {
                agent_id: "codex".into(),
                project_path: Some(r"D:\work\active".into()),
                process_ids: Vec::new(),
                started_at_ms: 20_000,
            },
            process_state: ProcessState::Stopped,
        };
        let old_history = session_candidate(
            "old-history",
            r"D:\work\active",
            20_000,
            SessionLifecycle::Historical,
            EventKind::TurnFailed,
        );

        let snapshot = build_snapshot(100_000, &[process], &[old_history]);

        let agent = &snapshot.agents[0];
        assert!(agent.active_session.is_none());
        assert_eq!(agent.display_status, DisplayStatus::Stopped);
        assert_eq!(agent.freshness.observed_at_ms, 100_000);
        assert!(!agent.freshness.stale);
    }

    #[test]
    fn retained_old_codex_snapshot_is_stale_after_its_adapter_window() {
        let process = ProcessFact {
            name: "Codex CLI".into(),
            identity: ProcessIdentity {
                agent_id: "codex".into(),
                project_path: Some(r"D:\work\active".into()),
                process_ids: vec![42],
                started_at_ms: 10_000,
            },
            process_state: ProcessState::Running,
        };
        let retained = session_candidate(
            "retained",
            r"D:\work\active",
            20_000,
            SessionLifecycle::Active,
            EventKind::TurnFailed,
        );

        let snapshot = build_snapshot(50_001, &[process], &[retained]);

        assert_eq!(snapshot.agents[0].freshness.observed_at_ms, 20_000);
        assert!(snapshot.agents[0].freshness.stale);
    }
}
