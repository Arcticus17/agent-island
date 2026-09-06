# Group D2 implementation report

- Base: `7cbaeb2`.
- Scope: phase-review Important 8 and Minor 1 only.
- Files: `src-tauri/src/lib.rs`, `src-tauri/src/interface/snapshot.rs`.

## Corrected behavior

- `ProcessFact` carries a bounded `Busy`, `Idle`, or `Unknown` process-activity observation. The scanner writes a current CPU-derived observation on every running process poll. Busy is valid for five seconds; an old observation is treated as unknown.
- Snapshot reduction uses that fact only for `opencode` and `hermes`, and only if no structured turn evidence exists. It can make the nonterminal state executing or leave it idle; it cannot make done, error, or waiting. Structured turn events win, and a stopped process returns stopped before any fallback. A busy conservative adapter without a matched session remains working in the compatibility projection instead of being overwritten to idle.
- The additive v1 `active_turn` snapshot field exposes the structured `{agent_id, session_id, turn_id}` selected by the existing Group A reducer. It is not inferred from output text.
- Runtime seconds remain process-accounted. Done/error increments now run after snapshot reduction, only for a fresh, running agent whose active session exactly equals the structured active-turn session. A persisted key of agent/session/turn/status/result timestamp de-duplicates a repeated poll; a new turn with the same terminal status increments independently. Stale, unmatched, historical, ambiguous, and stopped snapshots do not increment counts.

## TDD evidence

RED was captured before the behavior implementation:

1. `opencode_busy_and_idle_activity_are_nonterminal_fallbacks` failed with `Idle` instead of `Executing` for a fresh Busy activity fact.
2. `terminal_statistics_deduplicate_same_poll_but_count_a_new_turn` failed at its first fresh matched terminal transition while the production helper returned false.
3. `busy_conservative_process_without_a_match_is_not_projected_as_idle` failed with `Idle` instead of `Working`.

GREEN coverage verifies Busy/Idle fallback, structured terminal precedence, stopped precedence, unmatched busy projection, same-turn repeat poll, distinct new turn with equal status, stale terminal, mismatched/unrelated identity, stopped process, and a newer historical terminal record.

## Verification

- Focused Rust regressions passed after each GREEN step.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline`: 106 passed, 0 failed.
- `npm test`: 23 frontend tests and 106 Rust tests passed.
- `npm run build`: passed.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: passed.
- `git diff --check`: passed.

## Boundary / concern

The statistics de-duplication retains the latest terminal transition key per agent, which is sufficient for consecutive polling of the reducer-selected active turn. It intentionally does not turn activity or text hints into statistics events. Existing user changes to `src-tauri/Cargo.toml` and generated schemas were left unstaged and unmodified by this group.

## Review P2 follow-up — bounded opaque terminal identity retention

- Base: `8bcf18c`.
- Scope: D2 review P2 and the documented activity-freshness clock boundary only.
- Files: `src-tauri/src/lib.rs`, `src-tauri/src/interface/snapshot.rs`.

`AgentStats` now keeps the 128 most recent terminal transition SHA-256 hashes per agent. A terminal is counted only when its opaque hash is absent, so A -> B -> A and an ordinary repeated poll do not count again, while a distinct turn with the same terminal status does. The bounded insertion order gives an explicit 128-entry retention policy.

Old aggregate stats files with `last_transition_key` remain readable: loading converts that legacy raw key into a SHA-256 hash and discards the raw value. New aggregate stats files and the `AgentStats` frontend payload contain only `recent_terminal_hashes`; regression coverage asserts that representative agent/session/turn identifiers do not serialize. No raw stable identifiers are retained in the runtime `AgentStats` type.

Activity freshness now accepts an observation exactly 5,000 ms old, rejects 5,001 ms, and rejects a future observation timestamp.

### TDD evidence

RED regression source was added before implementation for A -> B -> A plus save/load/revisit and for the 5-second, expired, and future activity boundaries. In this worker environment, `cargo test` could not link either pre- or post-fix because the MSVC `link.exe` executable is absent, so assertion-level RED/GREEN output is unavailable here. The pre-fix control flow compared only `last_transition_key`; therefore the A -> B -> A test's third `assert!(!update...)` is the direct P2 reproduction. The activity test likewise fails against the prior `saturating_sub` behavior for the future timestamp.

### Verification

- `npm run test:frontend`: 23 passed, 0 failed.
- `npm run build`: passed.
- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: passed.
- `git diff --check`: passed.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib`, `cargo check --manifest-path src-tauri/Cargo.toml --lib --offline`, and Rust portion of `npm test`: blocked before tests by missing `link.exe` in this worker's MSVC toolchain. A controller with the established linker environment must run the Rust suite.
