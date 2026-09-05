# Task 8 report: structured diagnostics and stale-data protection

## Scope delivered

- Added stable `DataIssue` codes, per-adapter `Freshness`, parse-failure classification, and retained-snapshot stale detection.
- Kept adapter parsing isolated: malformed lines increment `skipped_lines`; valid neighboring events survive; zero valid structured events yields `parse_failed` without discarding the legacy session projection.
- Added same-generation diagnostic caching plus read-only `get_diagnostics` and caller-path `export_diagnostics(destination, state)` commands.
- Added a frontend snapshot-transition decision gate. Stale polled snapshots remain displayable but cannot update the transition baseline, notify, flash, or auto-jump. Realtime hook notifications remain on their independent event path.

## RED / GREEN evidence

- A RED: `cargo test --manifest-path src-tauri/Cargo.toml error_contract_tests --lib` failed because `parse_issue` did not exist. A GREEN: 4/4 passed after minimal implementation; later coverage also verifies adapter-specific freshness windows and actual file-scan isolation.
- B RED: stale snapshot test failed because `AgentView.freshness` was absent; diagnostic export tests failed because the diagnostic types and report builder were absent. B GREEN: diagnostic focused tests 7/7 and stale focused tests 4/4 passed; actual cache-generation and caller-selected export-path tests passed.
- C RED: frontend snapshot tests failed because `snapshotTransitionDecision` was not exported. C GREEN: snapshot-focused frontend tests passed 12/12, including stale suppression of notification, both flashes, auto-jump, and baseline replacement.

## Privacy self-review

The export serializer is an explicit whitelist. It emits only app/OS version, adapter, stable SHA-256 session hash, event type, issue code, counts, timestamps/freshness, project basename, and aggregate message length. Raw session IDs and project paths exist only in the non-serializable in-memory `DiagnosticRecord`; prompt/message bodies, credentials/tokens, commands, full paths, log paths, and recent output are never fields of the export DTO. Serialization tests assert that raw session ID and full-path prefixes are absent.

## Verification

- `npm test`: 12 frontend tests and 79 Rust library tests passed.
- `npm run build`: Vite production build passed.
- `cargo build --manifest-path src-tauri/Cargo.toml`: passed; final rerun after cleanup is recorded in the handoff.
- `git diff --check`: passed.

The two pre-existing generated schema changes under `src-tauri/gen/schemas/` are intentionally excluded from the Task 8 commit.
