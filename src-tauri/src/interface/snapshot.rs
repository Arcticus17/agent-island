use serde::Serialize;

use crate::application::session_registry::{match_active_session, SessionMatch};
use crate::domain::{
    derive_display_status, freshness_for_adapter, AgentState, AttentionState, ConversationMessage,
    DataIssue, DisplayStatus, DomainEvent, EventKind, Freshness, ProcessIdentity, ProcessState,
    SessionIdentity, SessionLifecycle, TurnState,
};
use crate::interface::diagnostics::DiagnosticView;

#[derive(Debug, Clone)]
pub struct ProcessFact {
    pub name: String,
    pub identity: ProcessIdentity,
    pub process_state: ProcessState,
    pub activity: ProcessActivity,
}

/// A short-lived observation of process work, deliberately separate from log events.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProcessActivity {
    Busy { observed_at_ms: u64 },
    Idle { observed_at_ms: u64 },
    Unknown,
}

const ACTIVITY_FRESHNESS_MS: u64 = 5_000;

impl ProcessActivity {
    fn is_busy_at(self, now_ms: u64) -> bool {
        matches!(self, Self::Busy { observed_at_ms } if observed_at_ms <= now_ms && now_ms - observed_at_ms <= ACTIVITY_FRESHNESS_MS)
    }
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
    pub active_turn: Option<ActiveTurnIdentity>,
    pub history_sessions: Vec<SessionSummary>,
    pub diagnostic: Option<DiagnosticView>,
    pub freshness: Freshness,
}

/// Structured turn identity for downstream transition consumers; never inferred from text.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct ActiveTurnIdentity {
    pub agent_id: String,
    pub session_id: String,
    pub turn_id: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionView {
    pub id: String,
    pub name: String,
    pub cwd: Option<String>,
    pub log_path: Option<String>,
    pub records: Vec<ConversationMessage>,
    pub recent_output: Vec<String>,
    pub current_file: Option<String>,
    pub log_status: Option<String>,
    pub alert: Option<String>,
    pub lifecycle: SessionLifecycle,
    pub last_active_at_ms: u64,
    pub display_status: DisplayStatus,
}

#[derive(Debug, Clone, Serialize)]
pub struct SessionSummary {
    pub id: String,
    pub name: String,
    pub cwd: Option<String>,
    pub log_path: Option<String>,
    pub records: Vec<ConversationMessage>,
    pub recent_output: Vec<String>,
    pub current_file: Option<String>,
    pub log_status: Option<String>,
    pub alert: Option<String>,
    pub lifecycle: SessionLifecycle,
    pub last_active_at_ms: u64,
    pub display_status: DisplayStatus,
}

impl From<&SessionView> for SessionSummary {
    fn from(session: &SessionView) -> Self {
        Self {
            id: session.id.clone(),
            name: session.name.clone(),
            cwd: session.cwd.clone(),
            log_path: session.log_path.clone(),
            records: session.records.clone(),
            recent_output: session.recent_output.clone(),
            current_file: session.current_file.clone(),
            log_status: session.log_status.clone(),
            alert: session.alert.clone(),
            lifecycle: session.lifecycle,
            last_active_at_ms: session.last_active_at_ms,
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
        .map(|process| build_agent_view(now_ms, process, session_candidates, None))
        .collect();

    AgentViewSnapshot {
        schema_version: 1,
        generated_at_ms: now_ms,
        agents,
    }
}

pub fn build_snapshot_with_acquisition(
    now_ms: u64,
    process_facts: &[ProcessFact],
    session_candidates: &[SessionCandidate],
    acquisition_issues: &[DiagnosticView],
) -> AgentViewSnapshot {
    AgentViewSnapshot {
        schema_version: 1,
        generated_at_ms: now_ms,
        agents: process_facts
            .iter()
            .map(|process| {
                build_agent_view(
                    now_ms,
                    process,
                    session_candidates,
                    acquisition_issues
                        .iter()
                        .find(|issue| issue.adapter == process.identity.agent_id),
                )
            })
            .collect(),
    }
}

fn build_agent_view(
    now_ms: u64,
    process: &ProcessFact,
    session_candidates: &[SessionCandidate],
    acquisition_issue: Option<&DiagnosticView>,
) -> AgentView {
    let agent_candidates: Vec<&SessionCandidate> = session_candidates
        .iter()
        .filter(|candidate| candidate.identity.agent_id == process.identity.agent_id)
        .collect();
    let identities: Vec<SessionIdentity> = agent_candidates
        .iter()
        .map(|candidate| candidate.identity.clone())
        .collect();
    let session_match =
        if process.process_state == ProcessState::Running && acquisition_issue.is_none() {
            match_active_session(&process.identity, &identities, now_ms)
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
    let mut freshness = freshness_for_adapter(&process.identity.agent_id, observed_at_ms, now_ms);
    freshness.stale |= acquisition_issue.is_some();
    let diagnostic = acquisition_issue
        .cloned()
        .map(|mut issue| {
            issue.freshness.stale = true;
            issue
        })
        .or_else(|| {
            diagnostic_issue.map(|issue| {
                DiagnosticView::new(&process.identity.agent_id, issue, freshness, None, 0, 0)
            })
        });
    let active_session = active_session_id.as_deref().and_then(|session_id| {
        agent_candidates
            .iter()
            .find(|candidate| candidate.identity.session_id == session_id)
            .map(|candidate| session_view(now_ms, candidate, process, &agent_candidates))
    });
    let history_candidates = agent_candidates
        .iter()
        .filter(|candidate| {
            Some(candidate.identity.session_id.as_str()) != active_session_id.as_deref()
        })
        .copied()
        .collect::<Vec<_>>();
    let history_sessions = history_candidates
        .into_iter()
        .take(3)
        .map(|candidate| SessionSummary::from(&history_session_view(candidate, &agent_candidates)))
        .collect();
    let state = reduce_agent_state(
        now_ms,
        process,
        active_session_id.as_deref(),
        &agent_candidates,
    );
    let active_turn = active_turn_identity(
        &process.identity.agent_id,
        active_session_id.as_deref(),
        &agent_candidates,
    );

    AgentView {
        id: process.identity.agent_id.clone(),
        name: process.name.clone(),
        display_status: derive_display_status(&state, now_ms),
        state,
        active_session,
        active_turn,
        history_sessions,
        diagnostic,
        freshness,
    }
}

fn history_session_view(
    candidate: &SessionCandidate,
    candidates: &[&SessionCandidate],
) -> SessionView {
    let state = reduce_agent_state(
        candidate.identity.last_event_at_ms,
        &ProcessFact {
            name: String::new(),
            identity: ProcessIdentity {
                agent_id: candidate.identity.agent_id.clone(),
                project_path: None,
                process_ids: Vec::new(),
                started_at_ms: 0,
            },
            process_state: ProcessState::Running,
            activity: ProcessActivity::Unknown,
        },
        Some(&candidate.identity.session_id),
        candidates,
    );
    let mut view = candidate.view.clone();
    view.display_status = match state.attention {
        AttentionState::ApprovalRequired | AttentionState::InputRequired => DisplayStatus::Waiting,
        AttentionState::None => match state.turn {
            TurnState::Executing => DisplayStatus::Working,
            TurnState::Succeeded => DisplayStatus::Done,
            TurnState::Failed => DisplayStatus::Error,
            TurnState::Idle => DisplayStatus::Idle,
        },
    };
    view
}

fn session_view(
    now_ms: u64,
    candidate: &SessionCandidate,
    process: &ProcessFact,
    candidates: &[&SessionCandidate],
) -> SessionView {
    let state = reduce_agent_state(
        now_ms,
        process,
        Some(&candidate.identity.session_id),
        candidates,
    );
    let mut view = candidate.view.clone();
    view.display_status = derive_display_status(&state, now_ms);
    view
}

fn reduce_agent_state(
    now_ms: u64,
    process: &ProcessFact,
    active_session_id: Option<&str>,
    candidates: &[&SessionCandidate],
) -> AgentState {
    let mut state = AgentState {
        process: process.process_state,
        turn: TurnState::Idle,
        attention: AttentionState::None,
        result_at_ms: None,
    };
    if process.process_state == ProcessState::Stopped {
        return state;
    }
    let Some(active_session_id) = active_session_id else {
        apply_process_activity_fallback(now_ms, process, false, &mut state);
        return state;
    };
    let mut events: Vec<&DomainEvent> = candidates
        .iter()
        .flat_map(|candidate| candidate.events.iter())
        .filter(|event| event.session_id == active_session_id)
        .collect();
    events.sort_by_key(|event| event.at_ms);
    let active_turn_id = events
        .iter()
        .rev()
        .find(|event| matches!(event.kind, EventKind::TurnStarted))
        .or_else(|| events.last())
        .map(|event| event.turn_id.as_str());
    let has_structured_turn_evidence = events
        .iter()
        .any(|event| !matches!(event.kind, EventKind::DiagnosticHint { .. }));

    for event in events
        .into_iter()
        .filter(|event| Some(event.turn_id.as_str()) == active_turn_id)
    {
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
    apply_process_activity_fallback(now_ms, process, has_structured_turn_evidence, &mut state);
    state
}

fn apply_process_activity_fallback(
    now_ms: u64,
    process: &ProcessFact,
    has_structured_turn_evidence: bool,
    state: &mut AgentState,
) {
    if !has_structured_turn_evidence
        && matches!(process.identity.agent_id.as_str(), "opencode" | "hermes")
        && process.activity.is_busy_at(now_ms)
    {
        state.turn = TurnState::Executing;
        state.attention = AttentionState::None;
        state.result_at_ms = None;
    }
}

fn active_turn_identity(
    agent_id: &str,
    active_session_id: Option<&str>,
    candidates: &[&SessionCandidate],
) -> Option<ActiveTurnIdentity> {
    let session_id = active_session_id?;
    let mut events: Vec<&DomainEvent> = candidates
        .iter()
        .flat_map(|candidate| candidate.events.iter())
        .filter(|event| event.session_id == session_id)
        .collect();
    events.sort_by_key(|event| event.at_ms);
    let turn_id = events
        .iter()
        .rev()
        .find(|event| matches!(event.kind, EventKind::TurnStarted))
        .or_else(|| events.last())?
        .turn_id
        .clone();
    Some(ActiveTurnIdentity {
        agent_id: agent_id.into(),
        session_id: session_id.into(),
        turn_id,
    })
}

#[cfg(test)]
mod tests {
    use super::{build_snapshot, ProcessActivity, ProcessFact, SessionCandidate, SessionView};
    use crate::adapters::codex::CodexAdapter;
    use crate::adapters::AgentAdapter;
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
                records: Vec::new(),
                recent_output: Vec::new(),
                current_file: None,
                log_status: None,
                alert: None,
                lifecycle,
                last_active_at_ms: last_event_at_ms,
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
            activity: ProcessActivity::Unknown,
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
            DisplayStatus::Error
        );
    }

    #[test]
    fn serialized_history_keeps_its_own_output_metadata_and_terminal_result() {
        let process = ProcessFact {
            name: "Codex CLI".into(),
            identity: ProcessIdentity {
                agent_id: "codex".into(),
                project_path: Some(r"D:\work\active".into()),
                process_ids: Vec::new(),
                started_at_ms: 20_000,
            },
            process_state: ProcessState::Stopped,
            activity: ProcessActivity::Unknown,
        };
        let mut historical = session_candidate(
            "old-session",
            r"D:\work\old",
            30_000,
            SessionLifecycle::Historical,
            EventKind::TurnSucceeded,
        );
        historical.view.log_path = Some(r"D:\logs\old.jsonl".into());
        historical.view.recent_output = vec!["old prompt".into(), "old result".into()];
        historical.view.current_file = Some(r"D:\work\old\main.rs".into());
        historical.view.log_status = Some("done".into());
        historical.view.alert = Some("completed".into());

        let snapshot = build_snapshot(31_000, &[process], &[historical]);
        let history = serde_json::to_value(&snapshot.agents[0].history_sessions[0]).unwrap();

        assert_eq!(history["display_status"], "done");
        assert_eq!(
            history["recent_output"],
            serde_json::json!(["old prompt", "old result"])
        );
        assert_eq!(history["log_path"], r#"D:\logs\old.jsonl"#);
        assert_eq!(history["current_file"], r#"D:\work\old\main.rs"#);
        assert_eq!(history["log_status"], "done");
        assert_eq!(history["lifecycle"], "Historical");
        assert_eq!(history["last_active_at_ms"], 30_000);
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
            activity: ProcessActivity::Unknown,
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
        assert_eq!(
            agent
                .history_sessions
                .iter()
                .map(|session| session.display_status)
                .collect::<Vec<_>>(),
            vec![DisplayStatus::Working, DisplayStatus::Waiting]
        );
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
            activity: ProcessActivity::Unknown,
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
            activity: ProcessActivity::Unknown,
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
            activity: ProcessActivity::Unknown,
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

    #[test]
    fn adapter_events_from_an_old_turn_do_not_override_the_newer_turn_snapshot() {
        let report = CodexAdapter.parse(concat!(
            r#"{"timestamp":"2026-09-04T10:00:00Z","type":"session_meta","payload":{"id":"s"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:00Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-a"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:01Z","type":"event_msg","payload":{"type":"task_started","turn_id":"turn-b"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:02Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-b"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:03Z","type":"event_msg","payload":{"type":"task_complete","turn_id":"turn-a"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:04Z","type":"event_msg","payload":{"type":"exec_approval_request","turn_id":"turn-a"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:05Z","type":"response_item","payload":{"type":"function_call","turn_id":"turn-a","call_id":"a-call","name":"tool","arguments":"{}"}}"#,
            "\n",
            r#"{"timestamp":"2026-09-04T10:00:06Z","type":"response_item","payload":{"type":"function_call_output","turn_id":"turn-a","call_id":"a-call","output":"Process exited with code 0"}}"#,
        ));
        let process = ProcessFact {
            name: "Codex CLI".into(),
            identity: ProcessIdentity {
                agent_id: "codex".into(),
                project_path: Some(r"D:\work\active".into()),
                process_ids: vec![42],
                started_at_ms: 0,
            },
            process_state: ProcessState::Running,
            activity: ProcessActivity::Unknown,
        };
        let last_event_at_ms = report
            .events
            .iter()
            .map(|event| event.at_ms)
            .max()
            .expect("fixture must include events");
        let candidate = SessionCandidate {
            identity: SessionIdentity {
                agent_id: "codex".into(),
                session_id: "s".into(),
                project_path: Some(r"D:\work\active".into()),
                process_ids: vec![42],
                started_at_ms: 0,
                last_event_at_ms,
                source: EventSource::CodexLog,
                confidence: Confidence::Confirmed,
                lifecycle: SessionLifecycle::Active,
            },
            view: SessionView {
                id: "s".into(),
                name: "s".into(),
                cwd: Some(r"D:\work\active".into()),
                log_path: None,
                records: Vec::new(),
                recent_output: Vec::new(),
                current_file: None,
                log_status: None,
                alert: None,
                lifecycle: SessionLifecycle::Active,
                last_active_at_ms: last_event_at_ms,
                display_status: DisplayStatus::Idle,
            },
            events: report.events,
        };

        let snapshot = build_snapshot(last_event_at_ms + 1_000, &[process], &[candidate]);

        assert_eq!(snapshot.agents[0].state.turn, TurnState::Succeeded);
        assert_eq!(snapshot.agents[0].state.attention, AttentionState::None);
        assert_eq!(snapshot.agents[0].display_status, DisplayStatus::Done);
    }

    #[test]
    fn opencode_busy_and_idle_activity_are_nonterminal_fallbacks() {
        let mut process = ProcessFact {
            name: "OpenCode".into(),
            identity: ProcessIdentity {
                agent_id: "opencode".into(),
                project_path: Some(r"D:\work\active".into()),
                process_ids: vec![42],
                started_at_ms: 0,
            },
            process_state: ProcessState::Running,
            activity: ProcessActivity::Busy {
                observed_at_ms: 10_000,
            },
        };
        let mut candidate = session_candidate(
            "opencode-active",
            r"D:\work\active",
            10_000,
            SessionLifecycle::Active,
            EventKind::DiagnosticHint {
                code: "text_never_terminal".into(),
            },
        );
        candidate.identity.agent_id = "opencode".into();
        candidate.events[0].agent_id = "opencode".into();

        let busy = build_snapshot(10_001, &[process.clone()], &[candidate.clone()]);
        assert_eq!(busy.agents[0].state.turn, TurnState::Executing);
        assert_eq!(busy.agents[0].display_status, DisplayStatus::Working);

        process.activity = ProcessActivity::Idle {
            observed_at_ms: 10_001,
        };
        let idle = build_snapshot(10_002, &[process], &[candidate]);
        assert_eq!(idle.agents[0].state.turn, TurnState::Idle);
        assert_eq!(idle.agents[0].display_status, DisplayStatus::Idle);
    }

    #[test]
    fn activity_freshness_accepts_exactly_five_seconds_but_rejects_expired_and_future_observations()
    {
        let activity = ProcessActivity::Busy {
            observed_at_ms: 10_000,
        };

        assert!(activity.is_busy_at(15_000));
        assert!(!activity.is_busy_at(15_001));
        assert!(!activity.is_busy_at(9_999));
    }

    #[test]
    fn structured_turn_and_stopped_process_override_activity_fallback() {
        let mut process = ProcessFact {
            name: "Hermes".into(),
            identity: ProcessIdentity {
                agent_id: "hermes".into(),
                project_path: Some(r"D:\work\active".into()),
                process_ids: vec![42],
                started_at_ms: 0,
            },
            process_state: ProcessState::Running,
            activity: ProcessActivity::Busy {
                observed_at_ms: 10_000,
            },
        };
        let mut candidate = session_candidate(
            "hermes-active",
            r"D:\work\active",
            10_000,
            SessionLifecycle::Active,
            EventKind::TurnSucceeded,
        );
        candidate.identity.agent_id = "hermes".into();
        candidate.events[0].agent_id = "hermes".into();

        let structured = build_snapshot(10_001, &[process.clone()], &[candidate.clone()]);
        assert_eq!(structured.agents[0].state.turn, TurnState::Succeeded);
        assert_eq!(structured.agents[0].display_status, DisplayStatus::Done);

        process.process_state = ProcessState::Stopped;
        let stopped = build_snapshot(10_001, &[process], &[candidate]);
        assert_eq!(stopped.agents[0].display_status, DisplayStatus::Stopped);
    }

    #[test]
    fn busy_conservative_process_without_a_match_is_not_projected_as_idle() {
        let process = ProcessFact {
            name: "OpenCode".into(),
            identity: ProcessIdentity {
                agent_id: "opencode".into(),
                project_path: Some(r"D:\work\unknown".into()),
                process_ids: vec![42],
                started_at_ms: 0,
            },
            process_state: ProcessState::Running,
            activity: ProcessActivity::Busy {
                observed_at_ms: 10_000,
            },
        };

        let snapshot = build_snapshot(10_001, &[process], &[]);

        assert!(snapshot.agents[0].active_session.is_none());
        assert_eq!(snapshot.agents[0].display_status, DisplayStatus::Working);
    }
}
