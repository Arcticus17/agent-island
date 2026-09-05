# Task 6 Report: Versioned Agent Snapshot

## Status

Implemented the versioned snapshot boundary and preserved the legacy `get_agents` command.

## Implementation

- Added `interface::snapshot` with schema version 1 DTOs, narrow `ProcessFact`/`SessionCandidate` inputs, and `build_snapshot`.
- Bound agent state and display status only to the session selected by `match_active_session`; newer unrelated history cannot override it.
- Ambiguous matches publish no active session and emit the minimal `session_ambiguous` diagnostic code.
- Added per-session display status derived from that session's structured events.
- Wired the existing scanner through the Claude/Codex/OpenCode/Hermes adapters without treating legacy text keywords as terminal state.
- Added additive legacy JSON fields `display_status`, `active_session`, and `history_sessions`; `session_list` now contains only the active session.
- Cached the immutable snapshot beside the legacy projection and registered `get_agent_snapshot` while retaining `get_agents`.

## TDD Evidence

- RED: `snapshot_uses_matching_session_not_newest_history` failed to compile because the snapshot builder and DTOs were absent.
- GREEN: the focused test passed after the minimal builder.
- RED/GREEN: ambiguity diagnostic, per-session status, legacy projection, and structured-adapter wiring each failed before their implementations and passed afterward.

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml snapshot_uses_matching_session_not_newest_history --lib`: 1 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml legacy_ --lib`: 2 passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 61 passed, 0 failed.
- `cargo build --manifest-path src-tauri/Cargo.toml`: passed.
- `rustfmt --edition 2021 src-tauri/src/interface/mod.rs src-tauri/src/interface/snapshot.rs`: passed.
- `git diff --check`: passed (Git emitted only existing line-ending/config warnings).

## Self-review and Concerns

- The two pre-existing generated schema files remain modified but were neither edited intentionally nor staged.
- Legacy sessions without readable structured logs remain conservatively idle/unknown; filename metadata is used only for freshness matching, never for terminal state.
- `DiagnosticView` intentionally contains only `code`; Task 8 may re-export or extend it without changing this DTO contract.

## Review Remediation Takeover (2026-09-05)

Resumed from commit `32e1124` with the predecessor's uncommitted `lib.rs` work intact. The initial `cargo test --manifest-path src-tauri/Cargo.toml --lib --no-run` compiled successfully; inspection showed that the remaining failure was production data flow rather than a compiler break.

- Replaced the production legacy-DTO round trip with direct `scan_agents` assembly from real sysinfo process observations and each adapter's `SessionScan.candidates`.
- Process facts now retain every observed PID, use the latest real process start time as conservative evidence, and keep cwd absent when observations are missing or conflict. Historical session cwd is never inserted into process identity.
- Preserved the predecessor's canonical Codex metadata id, bounded head-plus-tail reader, large-log regression, Claude/OpenCode fixture conversions, and Hermes `ParseReport.sessions` conversion, and connected those candidates to the production snapshot builder.
- Moved the shared cache timestamp sample to after scanning. An injected clock simulates a two-second scan and proves a legacy/snapshot follow-up 100 ms later reuses one generation.
- Forced every unmatched history summary to `idle`, including ambiguous candidates still marked `Active` whose events otherwise reduce to `working` or `waiting`.
- Removed the obsolete legacy snapshot reconstruction and unreachable structured-event parsing path after production wiring superseded them.

### Remediation TDD Evidence

- RED: `cache_timestamp_is_sampled_after_a_slow_scan` initially failed to compile because the injectable cache boundary did not exist.
- RED: `ambiguous_sessions_have_no_active_session_and_emit_a_diagnostic` failed when active-lifecycle ambiguous candidates leaked non-idle history status.
- GREEN: both regressions passed after the minimal cache and history changes, together with the process-fact, Codex/Claude/OpenCode, long-log, and Hermes focused tests.

### Fresh Verification

- `cargo test --manifest-path src-tauri/Cargo.toml --lib`: 68 passed, 0 failed.
- `cargo build --manifest-path src-tauri/Cargo.toml`: passed without warnings.
- `git diff --check`: passed; Git emitted only line-ending warnings for the two pre-existing generated schema differences and edited Rust files.

The pre-existing `src-tauri/gen/schemas/desktop-schema.json` and `src-tauri/gen/schemas/windows-schema.json` changes were not staged or committed.
