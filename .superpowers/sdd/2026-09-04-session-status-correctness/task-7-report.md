# Task 7 Report: Session-Consistent Legacy Views

## Status

Implemented session-consistent selectors for the legacy island and overview UI.

## Implementation

- Added `islandSession(agent)`, which returns only `active_session`; an absent or explicit-null active session never falls back to history or `session_list`.
- Added `overviewRows(agents)`, which flattens projected active and historical sessions, keeps the active copy when an ID is duplicated, and deduplicates by agent identity plus session ID.
- Limited old `session_list` compatibility to overview data that has neither projection field.
- Added `statusFor(agent, session)`, preferring `session.display_status`, then legacy session status, before agent status fields.
- Migrated the island render, notification transition, preview, path actions, terminal action, paging, and poll retention away from `session_list[sessionIdx]`.
- Bound island notifications to a per-agent/per-active-session status key so a newly selected active session cannot inherit another session's transition.
- Cleared session output and paths when no active session is confirmed and displayed `无法确认当前会话` instead of borrowing legacy agent log data.
- Replaced overview row construction with the shared selector; historical rows no longer borrow current agent output, cwd, or file values.
- Updated both demo fixtures to explicit `active_session`, `history_sessions`, and `display_status` fields.
- Added frontend and combined frontend/Rust npm test scripts.

## TDD Evidence

- RED: `node --test tests/session-view.test.mjs` failed with `ERR_MODULE_NOT_FOUND` for the intentionally absent `src/session-view.js`.
- GREEN: the same focused command passed 6 selector tests after the minimal selector implementation.
- The tests cover explicit-null island behavior, active-session selection, active/history deduplication, cross-agent ID independence, migration fallback boundaries, status precedence, and prevention of historical agent-field borrowing.

## Verification

- `node --test tests/session-view.test.mjs`: 6 passed, 0 failed.
- `npm run test:frontend`: 7 passed, 0 failed, including the existing layout test.
- `npm test`: frontend 7 passed and Rust library 68 passed, 0 failed.
- `npm run build`: Vite production build passed; 15 modules transformed.
- `node --check src/main.js` and `node --check src/overview.js`: passed.
- `git diff --check`: passed with only existing line-ending/config warnings.

## Self-review and Concerns

- The island intentionally no longer pages through historical sessions; its paging controls remain disabled because only one confirmed active session is safe to show.
- Old payloads without projection fields remain visible in overview only. They intentionally do not re-enable island fallback.
- Historical summaries contain no output or current-file fields in the new schema, so the overview truthfully shows no output/path rather than displaying the current agent's data.
- Task 8 remains responsible for stale snapshot gating.
- The two pre-existing generated schema files remain modified but were not edited, staged, or committed by this task.

## Review Remediation Round 1

- Added stable `id` to the production `AgentInfo` legacy DTO, populated it from the scanner's existing `agent_id`, and matched snapshot projections by ID rather than mutable display name.
- Added `agentKey`, `sessionKey`, and `restoreAgentIndex` selectors. New payloads use stable IDs; old ID-less payloads use a name plus stable-position compatibility key that keeps duplicate names distinct.
- Changed poll restoration to retain the selected agent by ID across sorting and display-name changes.
- Changed output cache and notification transition keys to combine stable agent identity with active session ID, preventing collisions between equal display names and equal session IDs.
- Changed notification alert lookup and agent-strip active/click targeting to stable identity. Existing name-based pinned and ordering preferences remain unchanged for compatibility.
- Confirmed overview row IDs remain isolated by stable agent ID and session ID even when display names and session IDs are duplicated.

### Remediation TDD Evidence

- RED: importing `agentKey`, `sessionKey`, and `restoreAgentIndex` failed because those selectors did not exist.
- GREEN: focused frontend selector tests passed 9/9 after adding stable keys and poll restoration.
- RED: the legacy projection serialization test returned `null` for `id` before the production DTO exposed it.
- RED: after adding the field, a snapshot with the same ID but renamed display name remained `idle`, proving projection still matched by name.
- GREEN: the focused Rust projection test passed after matching by stable ID and serialized `id` as `codex`.

### Fresh Remediation Verification

- `cargo fmt --manifest-path src-tauri/Cargo.toml -- --check`: passed.
- `npm test`: frontend 10 passed and Rust library 68 passed, 0 failed.
- `npm run build`: Vite production build passed; 15 modules transformed.
- `node --check src/main.js` and `node --check src/overview.js`: passed.
- `git diff --check`: passed with only existing line-ending/config warnings.

The two pre-existing generated schema differences remain unstaged. Task 8 stale gating and name-based pinned/ordering preference migration remain outside this repair.
