pub mod claude;

use crate::domain::{ConversationMessage, DomainEvent};

pub trait AgentAdapter {
    fn parse(&self, text: &str) -> ParseReport;
}

#[derive(Debug, Default, PartialEq, Eq)]
pub struct ParseReport {
    pub events: Vec<DomainEvent>,
    pub messages: Vec<ConversationMessage>,
    pub skipped_lines: usize,
}
