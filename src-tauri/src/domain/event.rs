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
    /// 工具名（Bash / Edit / Read …）；普通消息为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tool: Option<String>,
    /// 工具的关键入参（命令、文件路径、搜索词），回执里当一行摘要用。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
    /// 工具结果是否成功；结果还没到或不是工具消息时为空。
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub success: Option<bool>,
}
