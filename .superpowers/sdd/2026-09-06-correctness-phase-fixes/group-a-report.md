# Group A report — turn identity isolation

## Scope

- `src-tauri/src/adapters/codex.rs`
- `src-tauri/src/interface/snapshot.rs`

## Root cause

The Codex adapter assigned an explicit `task_complete.turn_id` to its mutable
current-turn context. A delayed completion for turn A therefore caused later
implicit events to be emitted for A instead of the active turn B. Snapshot
reduction sorted events only by session and applied all of them, so delayed
terminal, attention, and tool events for A could overwrite B.

## TDD evidence

RED was captured before production edits:

- `explicit_old_task_complete_does_not_rewind_active_turn_context` failed with
  emitted turns `A, B, A, A, A` instead of `A, B, A, B, B`.
- `adapter_events_from_an_old_turn_do_not_override_the_newer_turn_snapshot`
  failed with `Executing` instead of B's `Succeeded` / `Done` state.

The minimal implementation resolves an explicit turn ID per event without
rewriting the active parser context, including tool-call pairing. The reducer
selects the latest `TurnStarted` turn and applies only that turn's events. If a
truncated stream contains no turn start, it preserves prior same-turn behavior
by reducing the newest event's turn.

## GREEN verification

- Focused RED regressions: both passed after the fix.
- `cargo test --manifest-path src-tauri/Cargo.toml --lib --offline`: 84 passed.
- `npm test`: 17 frontend tests and 84 Rust tests passed.
- `npm run build`: passed.
- `cargo fmt --manifest-path src-tauri/Cargo.toml --check`: passed.
- `git diff --check`: passed.

## Concerns

The reducer intentionally defines the active turn as the latest structured
`TurnStarted` by timestamp. It cannot distinguish a genuinely delayed second
`TurnStarted` for an old turn from a newer active turn because `DomainEvent`
does not carry ingestion order; this is outside the reviewed failure mode,
which concerns delayed terminal, attention, and tool events.
