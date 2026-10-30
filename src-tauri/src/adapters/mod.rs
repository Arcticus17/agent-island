pub mod claude;
pub mod codex;
pub mod hermes;
pub mod opencode;

use crate::domain::{ConversationMessage, DomainEvent, SessionIdentity};

pub trait AgentAdapter {
    fn parse(&self, text: &str) -> ParseReport;
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParseReport {
    pub events: Vec<DomainEvent>,
    pub messages: Vec<ConversationMessage>,
    pub sessions: Vec<SessionIdentity>,
    pub skipped_lines: usize,
}

/// 有日志适配器的内置 Agent。自定义 Agent（例如 Pi、Gemini CLI）只有进程/状态能力：
/// 没有会话、日志与用量，前端要据此把界面话术改成"只监控进程"。
pub(crate) fn has_log_adapter(agent_id: &str) -> bool {
    matches!(agent_id, "claude" | "codex" | "opencode" | "hermes")
}

#[cfg(test)]
mod tests {
    use super::has_log_adapter;

    #[test]
    fn only_builtin_agents_have_log_adapters() {
        for id in ["claude", "codex", "opencode", "hermes"] {
            assert!(has_log_adapter(id), "{id} 应该有日志适配器");
        }
        for id in ["pi", "gemini", "aider", "custom", ""] {
            assert!(!has_log_adapter(id), "{id} 不应被当成适配器 Agent");
        }
    }
}
