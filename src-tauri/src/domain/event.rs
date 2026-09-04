use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventSource {
    ClaudeHook,
    ClaudeLog,
    CodexLog,
    Process,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Confidence {
    Confirmed,
    Probable,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventKind {
    TurnStarted,
    ToolStarted,
    ToolFinished { success: bool },
    AttentionRequested { approval: bool },
    TurnSucceeded,
    TurnFailed,
    DiagnosticHint { code: String },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DomainEvent {
    pub agent_id: String,
    pub session_id: String,
    pub turn_id: String,
    pub at_ms: u64,
    pub source: EventSource,
    pub confidence: Confidence,
    pub kind: EventKind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageRole {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub event_id: String,
    pub at_ms: u64,
    pub role: MessageRole,
    pub text: String,
}
