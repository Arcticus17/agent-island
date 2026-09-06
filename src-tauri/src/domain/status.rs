use serde::{Deserialize, Serialize};

use super::EVENT_CLOCK_SKEW_TOLERANCE_MS;

pub const DONE_HIGHLIGHT_MS: u64 = 8_000;
pub const ERROR_HIGHLIGHT_MS: u64 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessState {
    Stopped,
    Running,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState {
    Idle,
    Executing,
    Succeeded,
    Failed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionState {
    None,
    InputRequired,
    ApprovalRequired,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayStatus {
    Stopped,
    Idle,
    Working,
    Done,
    Error,
    Waiting,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentState {
    pub process: ProcessState,
    pub turn: TurnState,
    pub attention: AttentionState,
    pub result_at_ms: Option<u64>,
}

impl AgentState {
    pub fn running(turn: TurnState, attention: AttentionState) -> Self {
        Self {
            process: ProcessState::Running,
            turn,
            attention,
            result_at_ms: None,
        }
    }

    pub fn result(turn: TurnState, at_ms: u64) -> Self {
        Self {
            process: ProcessState::Running,
            turn,
            attention: AttentionState::None,
            result_at_ms: Some(at_ms),
        }
    }
}

pub fn derive_display_status(state: &AgentState, now_ms: u64) -> DisplayStatus {
    if state.process == ProcessState::Stopped {
        return DisplayStatus::Stopped;
    }
    if state.attention != AttentionState::None {
        return DisplayStatus::Waiting;
    }
    match state.turn {
        TurnState::Executing => DisplayStatus::Working,
        TurnState::Succeeded
            if result_is_current(state.result_at_ms, now_ms, DONE_HIGHLIGHT_MS) =>
        {
            DisplayStatus::Done
        }
        TurnState::Failed if result_is_current(state.result_at_ms, now_ms, ERROR_HIGHLIGHT_MS) => {
            DisplayStatus::Error
        }
        _ => DisplayStatus::Idle,
    }
}

fn result_is_current(result_at_ms: Option<u64>, now_ms: u64, highlight_ms: u64) -> bool {
    let Some(result_at_ms) = result_at_ms else {
        return false;
    };
    if result_at_ms > now_ms {
        return result_at_ms - now_ms <= EVENT_CLOCK_SKEW_TOLERANCE_MS;
    }
    now_ms - result_at_ms <= highlight_ms
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stopped_process_dominates_stale_attention() {
        let state = AgentState {
            process: ProcessState::Stopped,
            turn: TurnState::Failed,
            attention: AttentionState::ApprovalRequired,
            result_at_ms: Some(1_000),
        };
        assert_eq!(derive_display_status(&state, 2_000), DisplayStatus::Stopped);
    }

    #[test]
    fn attention_dominates_running_turn_state() {
        let state = AgentState::running(TurnState::Executing, AttentionState::InputRequired);
        assert_eq!(derive_display_status(&state, 2_000), DisplayStatus::Waiting);
    }

    #[test]
    fn result_highlights_expire() {
        let done = AgentState::result(TurnState::Succeeded, 1_000);
        let error = AgentState::result(TurnState::Failed, 1_000);
        assert_eq!(derive_display_status(&done, 8_999), DisplayStatus::Done);
        assert_eq!(derive_display_status(&done, 9_001), DisplayStatus::Idle);
        assert_eq!(derive_display_status(&error, 15_999), DisplayStatus::Error);
        assert_eq!(derive_display_status(&error, 16_001), DisplayStatus::Idle);
    }

    #[test]
    fn result_highlights_allow_small_clock_skew_but_reject_distant_future_events() {
        let tolerated = AgentState::result(TurnState::Succeeded, 7_000);
        let future = AgentState::result(TurnState::Failed, 7_001);

        assert_eq!(
            derive_display_status(&tolerated, 2_000),
            DisplayStatus::Done
        );
        assert_eq!(derive_display_status(&future, 2_000), DisplayStatus::Idle);
    }
}
