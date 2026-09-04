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
