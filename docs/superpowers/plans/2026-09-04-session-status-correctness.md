# Session and Status Correctness Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make Agent Island show only the active Claude/Codex session, preserve history only in the overview, and derive status from session-scoped structured events instead of stale logs or text keywords.

**Architecture:** Add pure domain event and status modules, Agent-specific adapters, and a session registry between raw logs/processes and the Tauri DTO. Keep the existing frontend operational through a compatibility DTO while exposing active and historical sessions separately.

**Tech Stack:** Rust 2021, Tauri 2, serde/serde_json, sysinfo 0.33, Node test runner for legacy frontend selectors.

**Spec:** `docs/superpowers/specs/2026-09-04-agent-island-reliability-flexible-ui-design.md`

## Global Constraints

- Windows remains the only release target in this plan.
- Keep Tauri 2 and Rust; do not introduce a background service.
- A text keyword may create a diagnostic hint but must never set a terminal turn result.
- The island shows only a reliably matched active session; ambiguous matches display “无法确认当前会话”.
- Every defect fix starts with a failing regression test.
- Keep the existing `get_agents` command available until the Svelte migration plan is complete.
- Do not record prompt bodies, credentials, or full private paths in diagnostic output.

---

## File Structure

- Create `src-tauri/src/domain/mod.rs`: domain exports.
- Create `src-tauri/src/domain/event.rs`: normalized events and source confidence.
- Create `src-tauri/src/domain/status.rs`: orthogonal state and display derivation.
- Create `src-tauri/src/domain/session.rs`: session identity and lifecycle models.
- Create `src-tauri/src/adapters/mod.rs`: adapter exports and trait.
- Create `src-tauri/src/adapters/claude.rs`: Claude JSONL normalization.
- Create `src-tauri/src/adapters/codex.rs`: Codex JSONL normalization.
- Create `src-tauri/src/application/mod.rs`: application exports.
- Create `src-tauri/src/application/session_registry.rs`: active-session matching.
- Create `src-tauri/src/interface/mod.rs`: DTO exports.
- Create `src-tauri/src/interface/snapshot.rs`: versioned frontend snapshot.
- Create `src-tauri/tests/fixtures/claude/current-turn.jsonl`: successful Claude turn.
- Create `src-tauri/tests/fixtures/claude/historical-tool-error.jsonl`: recovered Claude tool failure.
- Create `src-tauri/tests/fixtures/codex/current-turn.jsonl`: successful Codex turn.
- Create `src-tauri/tests/fixtures/codex/recovered-command.jsonl`: recovered Codex command failure.
- Create `src-tauri/tests/fixtures/opencode/text-error.log`: text-only OpenCode diagnostic sample.
- Create `src-tauri/tests/fixtures/hermes/session-list.txt`: Hermes session-list sample.
- Create `src/session-view.js`: pure legacy view selectors.
- Create `tests/session-view.test.mjs`: legacy UI consistency tests.
- Modify `src-tauri/src/lib.rs`: wire modules into scanning and Tauri commands.
- Modify `src/main.js`: consume active-session selectors.
- Modify `src/overview.js`: consume historical-session selectors.
- Modify `package.json`: add a deterministic frontend test command.

### Task 1: Define the session-scoped state model

**Files:**
- Create: `src-tauri/src/domain/mod.rs`
- Create: `src-tauri/src/domain/event.rs`
- Create: `src-tauri/src/domain/status.rs`
- Create: `src-tauri/src/domain/session.rs`
- Modify: `src-tauri/src/lib.rs:1-16`

**Interfaces:**
- Produces: `DomainEvent`, `EventKind`, `EventSource`, `Confidence`, `AgentState`, `DisplayStatus`, `SessionIdentity`, `derive_display_status`.

- [ ] **Step 1: Write failing unit tests in `domain/status.rs`**

```rust
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
}
```

- [ ] **Step 2: Run the focused test and verify it fails**

Run: `cargo test --manifest-path src-tauri/Cargo.toml domain::status::tests --lib`

Expected: compilation fails because the domain module and types do not exist.

- [ ] **Step 3: Implement the minimal domain types**

```rust
// domain/status.rs
use serde::{Deserialize, Serialize};

pub const DONE_HIGHLIGHT_MS: u64 = 8_000;
pub const ERROR_HIGHLIGHT_MS: u64 = 15_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessState { Stopped, Running }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TurnState { Idle, Executing, Succeeded, Failed }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AttentionState { None, InputRequired, ApprovalRequired }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DisplayStatus { Stopped, Idle, Working, Done, Error, Waiting }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AgentState {
    pub process: ProcessState,
    pub turn: TurnState,
    pub attention: AttentionState,
    pub result_at_ms: Option<u64>,
}

impl AgentState {
    pub fn running(turn: TurnState, attention: AttentionState) -> Self {
        Self { process: ProcessState::Running, turn, attention, result_at_ms: None }
    }
    pub fn result(turn: TurnState, at_ms: u64) -> Self {
        Self { process: ProcessState::Running, turn, attention: AttentionState::None, result_at_ms: Some(at_ms) }
    }
}

pub fn derive_display_status(state: &AgentState, now_ms: u64) -> DisplayStatus {
    if state.process == ProcessState::Stopped { return DisplayStatus::Stopped; }
    if state.attention != AttentionState::None { return DisplayStatus::Waiting; }
    match state.turn {
        TurnState::Executing => DisplayStatus::Working,
        TurnState::Succeeded if now_ms.saturating_sub(state.result_at_ms.unwrap_or(0)) <= DONE_HIGHLIGHT_MS => DisplayStatus::Done,
        TurnState::Failed if now_ms.saturating_sub(state.result_at_ms.unwrap_or(0)) <= ERROR_HIGHLIGHT_MS => DisplayStatus::Error,
        _ => DisplayStatus::Idle,
    }
}
```

```rust
// domain/event.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EventSource { ClaudeHook, ClaudeLog, CodexLog, Process }

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Confidence { Confirmed, Probable, Unknown }

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
pub enum MessageRole { User, Assistant, Tool }

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConversationMessage {
    pub event_id: String,
    pub at_ms: u64,
    pub role: MessageRole,
    pub text: String,
}
```

```rust
// domain/session.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionLifecycle { Active, Historical }

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
```

Re-export all domain modules through `domain/mod.rs` and add `mod domain;` to `lib.rs`.

- [ ] **Step 4: Run tests and verify they pass**

Run: `cargo test --manifest-path src-tauri/Cargo.toml domain:: --lib`

Expected: all new domain tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/domain src-tauri/src/lib.rs
git commit -m "feat: add session-scoped agent state model"
```

### Task 2: Normalize Claude events without text-based terminal states

**Files:**
- Create: `src-tauri/src/adapters/mod.rs`
- Create: `src-tauri/src/adapters/claude.rs`
- Create: `src-tauri/tests/fixtures/claude/current-turn.jsonl`
- Create: `src-tauri/tests/fixtures/claude/historical-tool-error.jsonl`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: domain types from Task 1.
- Produces: `AgentAdapter::parse(&str) -> ParseReport`, `ClaudeAdapter`, `ParseReport { events, messages, skipped_lines }`.

- [ ] **Step 1: Add synthetic fixtures and failing adapter tests**

Write `historical-tool-error.jsonl` in this order so the older tool error cannot dominate the final result:

```jsonl
{"type":"user","sessionId":"claude-s1","uuid":"u1","timestamp":"2026-09-04T10:00:00Z","message":{"role":"user","content":[{"type":"text","text":"run checks"}]}}
{"type":"assistant","sessionId":"claude-s1","uuid":"a1","timestamp":"2026-09-04T10:00:01Z","message":{"role":"assistant","content":[{"type":"tool_result","is_error":true,"text":"command exited 1"}],"stop_reason":"tool_use"}}
{"type":"assistant","sessionId":"claude-s1","uuid":"a2","timestamp":"2026-09-04T10:00:02Z","message":{"role":"assistant","content":[{"type":"text","text":"recovered and verified"}],"stop_reason":"end_turn"}}
```

`current-turn.jsonl` uses the same field names with one user record and one successful assistant `end_turn` record. Tests must assert that message order is chronological and the final event is `TurnSucceeded`, not `TurnFailed`.

```rust
#[test]
fn historical_tool_error_does_not_fail_new_turn() {
    let report = ClaudeAdapter.parse(include_str!("../../tests/fixtures/claude/historical-tool-error.jsonl"));
    assert!(matches!(report.events.last().unwrap().kind, EventKind::TurnSucceeded));
    assert_eq!(report.messages.iter().map(|m| m.role).collect::<Vec<_>>(), [MessageRole::User, MessageRole::Assistant]);
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml adapters::claude::tests --lib`

Expected: compilation fails because `ClaudeAdapter` is missing.

- [ ] **Step 3: Implement Claude normalization**

Parse each complete JSONL line in file order. Emit messages only for explicit user/assistant message records. Emit `TurnStarted`, `ToolStarted`, `ToolFinished`, `AttentionRequested`, `TurnSucceeded`, or `TurnFailed` only from structured fields. Increment `skipped_lines` for malformed input. Never call the legacy `text_signal` function.

```rust
pub trait AgentAdapter {
    fn parse(&self, text: &str) -> ParseReport;
}

pub struct ParseReport {
    pub events: Vec<DomainEvent>,
    pub messages: Vec<ConversationMessage>,
    pub skipped_lines: usize,
}
```

- [ ] **Step 4: Run adapter and full Rust tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml adapters::claude::tests --lib`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Expected: both commands pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/adapters src-tauri/tests/fixtures/claude src-tauri/src/lib.rs
git commit -m "feat: normalize Claude session events"
```

### Task 3: Normalize Codex events and preserve event order

**Files:**
- Create: `src-tauri/src/adapters/codex.rs`
- Create: `src-tauri/tests/fixtures/codex/current-turn.jsonl`
- Create: `src-tauri/tests/fixtures/codex/recovered-command.jsonl`
- Modify: `src-tauri/src/adapters/mod.rs`

**Interfaces:**
- Consumes: `AgentAdapter`, `ParseReport`, domain types.
- Produces: `CodexAdapter` with the same output contract as `ClaudeAdapter`.

- [ ] **Step 1: Write fixtures and failing tests**

Write the recovered-command fixture with these records, then assert chronological message order, separate tool output records, and final success:

```jsonl
{"timestamp":"2026-09-04T10:00:00Z","type":"event_msg","payload":{"type":"user_message","message":"run checks"}}
{"timestamp":"2026-09-04T10:00:01Z","type":"response_item","payload":{"type":"function_call","call_id":"c1","name":"exec_command","arguments":"{\"command\":\"npm test\"}"}}
{"timestamp":"2026-09-04T10:00:02Z","type":"response_item","payload":{"type":"function_call_output","call_id":"c1","output":"Process exited with code 1"}}
{"timestamp":"2026-09-04T10:00:03Z","type":"response_item","payload":{"type":"message","role":"assistant","content":[{"type":"output_text","text":"fixed and rechecked"}]}}
{"timestamp":"2026-09-04T10:00:04Z","type":"event_msg","payload":{"type":"turn_complete"}}
```

```rust
#[test]
fn recovered_command_failure_finishes_successfully() {
    let report = CodexAdapter.parse(include_str!("../../tests/fixtures/codex/recovered-command.jsonl"));
    assert!(report.events.iter().any(|e| matches!(e.kind, EventKind::ToolFinished { success: false })));
    assert!(matches!(report.events.last().unwrap().kind, EventKind::TurnSucceeded));
}
```

- [ ] **Step 2: Run and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml adapters::codex::tests --lib`

Expected: compilation fails because `CodexAdapter` is missing.

- [ ] **Step 3: Implement Codex normalization**

Map `user_message`, assistant `message`, `function_call`, `function_call_output`, permission/approval events, turn-complete events, and explicit terminal error events. Ignore token-count events for conversation/status purposes; existing usage parsing remains unchanged.

- [ ] **Step 4: Run focused and full tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml adapters::codex::tests --lib`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/adapters/codex.rs src-tauri/src/adapters/mod.rs src-tauri/tests/fixtures/codex
git commit -m "feat: normalize Codex session events"
```

### Task 4: Normalize OpenCode and Hermes without inventing terminal state

**Files:**
- Create: `src-tauri/src/adapters/opencode.rs`
- Create: `src-tauri/src/adapters/hermes.rs`
- Create: `src-tauri/tests/fixtures/opencode/text-error.log`
- Create: `src-tauri/tests/fixtures/hermes/session-list.txt`
- Modify: `src-tauri/src/adapters/mod.rs`

**Interfaces:**
- Consumes: `AgentAdapter`, `ParseReport`, domain types.
- Produces: `OpenCodeAdapter` and `HermesAdapter`; both may return messages/diagnostics but no terminal result without structured evidence.

- [ ] **Step 1: Write failing tests for conservative fallback**

Create an OpenCode line containing `message=previous command error: recovered` and assert the report has a diagnostic hint but no `TurnFailed`. Create a Hermes list with header plus `project-a 2026-09-04 hermes-1` and assert one historical session with ID `hermes-1` and no terminal event.

- [ ] **Step 2: Run and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml adapters::opencode::tests adapters::hermes::tests --lib`

Expected: compilation fails because both adapters are missing.

- [ ] **Step 3: Implement conservative adapters**

OpenCode extracts timestamp, directory and display text but maps text signals only to `DiagnosticHint`; process activity later supplies executing/idle state. Hermes parses the CLI table into session identities and never treats the session name as conversation content or status evidence.

- [ ] **Step 4: Run focused and full tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml adapters:: --lib`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/adapters src-tauri/tests/fixtures/opencode src-tauri/tests/fixtures/hermes
git commit -m "feat: normalize fallback Agent sessions conservatively"
```

### Task 5: Match active sessions conservatively

**Files:**
- Create: `src-tauri/src/application/mod.rs`
- Create: `src-tauri/src/application/session_registry.rs`
- Modify: `src-tauri/src/lib.rs`

**Interfaces:**
- Consumes: `SessionIdentity`, normalized paths and process facts.
- Produces: `match_active_session(process: &ProcessIdentity, candidates: &[SessionIdentity]) -> SessionMatch`.

- [ ] **Step 1: Write failing matching tests**

```rust
#[test]
fn rejects_history_created_before_process_start() {
    let process = ProcessIdentity { agent_id: "codex".into(), project_path: Some(r"D:\work\app".into()), process_ids: vec![10], started_at_ms: 20_000 };
    let old = SessionIdentity { agent_id: "codex".into(), session_id: "old".into(), project_path: Some(r"D:\work\app".into()), process_ids: vec![], started_at_ms: 1_000, last_event_at_ms: 14_999, source: EventSource::CodexLog, confidence: Confidence::Unknown, lifecycle: SessionLifecycle::Historical };
    assert_eq!(match_active_session(&process, &[old]), SessionMatch::Unknown);
}

#[test]
fn accepts_unique_same_project_candidate() {
    let process = ProcessIdentity { agent_id: "codex".into(), project_path: Some(r"D:\work\app".into()), process_ids: vec![10], started_at_ms: 20_000 };
    let fresh = SessionIdentity { agent_id: "codex".into(), session_id: "fresh".into(), project_path: Some(r"d:\WORK\app\".into()), process_ids: vec![], started_at_ms: 19_000, last_event_at_ms: 21_000, source: EventSource::CodexLog, confidence: Confidence::Unknown, lifecycle: SessionLifecycle::Historical };
    assert_eq!(match_active_session(&process, &[fresh]), SessionMatch::Probable("fresh".into()));
}

#[test]
fn rejects_two_equally_valid_candidates() {
    let process = ProcessIdentity { agent_id: "codex".into(), project_path: Some(r"D:\work\app".into()), process_ids: vec![10], started_at_ms: 20_000 };
    let make = |id: &str| SessionIdentity { agent_id: "codex".into(), session_id: id.into(), project_path: Some(r"D:\work\app".into()), process_ids: vec![], started_at_ms: 19_000, last_event_at_ms: 21_000, source: EventSource::CodexLog, confidence: Confidence::Unknown, lifecycle: SessionLifecycle::Historical };
    assert_eq!(match_active_session(&process, &[make("one"), make("two")]), SessionMatch::Ambiguous(vec!["one".into(), "two".into()]));
}
```

Use exact expected variants `SessionMatch::Unknown`, `SessionMatch::Probable(session_id)`, and `SessionMatch::Ambiguous(Vec<String>)`.

- [ ] **Step 2: Run and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml application::session_registry::tests --lib`

Expected: compilation fails because the registry is missing.

- [ ] **Step 3: Implement deterministic matching**

Normalize Windows paths by trimming separators and comparing case-insensitively. Accept a probable match only when agent IDs and project paths match, the last event is at least `process.started_at_ms - 5_000`, and exactly one candidate remains. Return ambiguity with candidate IDs; never sort and choose the newest ambiguous candidate.

```rust
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionMatch {
    Confirmed(String),
    Probable(String),
    Ambiguous(Vec<String>),
    Unknown,
}
```

- [ ] **Step 4: Run focused and full tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml application::session_registry::tests --lib`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Expected: all tests pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/application src-tauri/src/lib.rs
git commit -m "feat: bind sessions to active processes conservatively"
```

### Task 6: Publish a versioned snapshot while preserving legacy commands

**Files:**
- Create: `src-tauri/src/interface/mod.rs`
- Create: `src-tauri/src/interface/snapshot.rs`
- Modify: `src-tauri/src/lib.rs:17-71,285-458,2034-2053,2972-2995`

**Interfaces:**
- Consumes: adapters, session registry, status engine.
- Produces: `build_snapshot(now_ms, process_facts, session_candidates) -> AgentViewSnapshot`, Tauri command `get_agent_snapshot()`, and legacy `get_agents()` projection.

- [ ] **Step 1: Write a failing integration-style unit test**

Construct one running Codex process for `D:\work\active`, one fresh matching session, and one newer unrelated historical session. Assert that `active_session.id` is the matching session, the unrelated session is only in `history_sessions`, and the derived status follows the matching session.

- [ ] **Step 2: Run and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml snapshot_uses_matching_session_not_newest_history --lib`

Expected: test fails because the snapshot builder is missing.

- [ ] **Step 3: Define the DTO and builder**

```rust
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
}
```

Set `schema_version` to `1`. For ambiguous sessions set `active_session` to `None` and diagnostic code `session_ambiguous`. Extend the compatibility `AgentInfo` DTO with optional `active_session` and `history_sessions` fields while retaining every existing field; place only the active session in legacy `session_list`.

- [ ] **Step 4: Register the new command and run tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Expected: all tests pass and `get_agents` remains registered alongside `get_agent_snapshot`.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/interface src-tauri/src/lib.rs
git commit -m "feat: expose versioned active-session snapshot"
```

### Task 7: Keep the legacy UI session-consistent

**Files:**
- Create: `src/session-view.js`
- Create: `tests/session-view.test.mjs`
- Modify: `src/main.js:400-520,1405-1454`
- Modify: `src/overview.js:67-125,562-590`
- Modify: `package.json:6-12`

**Interfaces:**
- Consumes: legacy projection fields `active_session`, `history_sessions`, `display_status`, with fallback support for old `session_list` during migration.
- Produces: `islandSession(agent)`, `overviewRows(agents)`, and `statusFor(agent, session)`.

- [ ] **Step 1: Write failing selector tests**

```js
import test from "node:test";
import assert from "node:assert/strict";
import { islandSession, overviewRows, statusFor } from "../src/session-view.js";

test("island never falls back to historical sessions", () => {
  const agent = { display_status: "idle", active_session: null, history_sessions: [{ id: "old" }] };
  assert.equal(islandSession(agent), null);
});

test("overview includes history while preserving session status", () => {
  const rows = overviewRows([{ id: "codex", name: "Codex CLI", history_sessions: [{ id: "old", display_status: "done" }] }]);
  assert.equal(rows[0].session.id, "old");
  assert.equal(statusFor(rows[0].agent, rows[0].session), "done");
});
```

- [ ] **Step 2: Run and verify failure**

Run: `node --test tests/session-view.test.mjs`

Expected: failure because `src/session-view.js` does not exist.

- [ ] **Step 3: Implement pure selectors and integrate them**

`islandSession` returns only `active_session`; `overviewRows` flattens active plus history with duplicate IDs removed; `statusFor` uses the selected session status before agent display status. Replace direct `session_list[sessionIdx]` and the current `rows()` implementation with these selectors. Add scripts:

```json
"test:frontend": "node --test tests/*.test.mjs",
"test": "npm run test:frontend && cargo test --manifest-path src-tauri/Cargo.toml --lib"
```

- [ ] **Step 4: Run frontend, Rust, and build verification**

Run: `npm test`

Run: `npm run build`

Expected: all tests pass and Vite production build succeeds.

- [ ] **Step 5: Commit**

```bash
git add src/session-view.js tests/session-view.test.mjs src/main.js src/overview.js package.json
git commit -m "fix: keep legacy views bound to one session"
```

### Task 8: Add structured diagnostics and stale-data protection

**Files:**
- Create: `src-tauri/src/domain/error.rs`
- Modify: `src-tauri/src/domain/mod.rs`
- Modify: `src-tauri/src/interface/snapshot.rs`
- Create: `src-tauri/src/interface/diagnostics.rs`
- Modify: `src-tauri/src/interface/mod.rs`
- Modify: `src-tauri/src/lib.rs`
- Test: module-local Rust tests.

**Interfaces:**
- Produces: `DataIssue`, `DiagnosticView`, `Freshness`, stale-snapshot suppression, `get_diagnostics`, and `export_diagnostics`.

- [ ] **Step 1: Write failing tests**

Assert one malformed line increments the skipped count without failing the adapter; a report with no valid structured events yields `ParseFailed`; a retained snapshot older than its adapter freshness window is marked stale and cannot emit a new notification.

- [ ] **Step 2: Run and verify failure**

Run: `cargo test --manifest-path src-tauri/Cargo.toml stale --lib`

Expected: tests fail because diagnostic types and stale behavior are missing.

- [ ] **Step 3: Implement structured errors**

```rust
#[derive(Debug, Clone, Serialize)]
#[serde(tag = "code", rename_all = "snake_case")]
pub enum DataIssue {
    LogUnavailable,
    ParseFailed { skipped_lines: usize },
    SessionAmbiguous { candidate_count: usize },
    ProcessGone,
    PermissionDenied,
}

#[derive(Debug, Clone, Serialize)]
pub struct Freshness { pub observed_at_ms: u64, pub stale: bool }
```

Prevent stale snapshots from entering the notification transition code. Log issue code, agent ID, session ID, source, and counts only.

Implement `get_diagnostics` as a read-only snapshot of current issue codes. Implement `export_diagnostics` to write a JSON report selected by the user containing app/OS versions, adapter name, hashed session ID, event type, issue code, counts and timestamps. Store only the project directory basename and message length; never store prompt bodies, credentials or full paths.

- [ ] **Step 4: Run complete verification**

Run: `npm test`

Run: `npm run build`

Expected: all checks pass.

- [ ] **Step 5: Commit**

```bash
git add src-tauri/src/domain src-tauri/src/interface src-tauri/src/lib.rs
git commit -m "feat: isolate adapter failures and stale data"
```

### Task 9: Perform Windows regression verification

**Files:**
- Modify: `README.md` only if behavior or diagnostics documentation is missing.
- Create: `docs/verification/2026-09-04-session-status.md` with command results and manual observations.

**Interfaces:**
- Consumes: all deliverables in this plan.
- Produces: evidence that the correctness phase is ready for frontend migration.

- [ ] **Step 1: Run automated checks**

Run: `npm test`

Run: `npm run build`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Expected: all commands exit successfully.

- [ ] **Step 2: Run the Tauri app against real local sessions**

Run: `npm run tauri dev`

Verify: two projects do not exchange logs; historical sessions appear only in overview; a repaired tool failure ends as done; an unmatched session shows “无法确认当前会话”; stopping a process shows stopped even if a stale approval exists.

- [ ] **Step 3: Record evidence without private content**

Write only command names, pass/fail, Agent type, synthetic session IDs, display state transitions, and whether a match was confirmed or ambiguous. Do not copy prompt text or full user paths.

- [ ] **Step 4: Commit verification evidence**

```bash
git add README.md docs/verification/2026-09-04-session-status.md
git commit -m "test: verify session and status correctness"
```
