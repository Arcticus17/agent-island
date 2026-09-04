use serde::{Deserialize, Serialize};

use super::{Confidence, EventSource};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionLifecycle {
    Active,
    Historical,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SessionIdentity {
    pub agent_id: String,
    pub session_id: String,
    pub project_path: Option<String>,
    pub process_ids: Vec<u32>,
    pub started_at_ms: u64,
    pub last_event_at_ms: u64,
    pub source: EventSource,
    pub confidence: Confidence,
    pub lifecycle: SessionLifecycle,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessIdentity {
    pub agent_id: String,
    pub project_path: Option<String>,
    pub process_ids: Vec<u32>,
    pub started_at_ms: u64,
}
