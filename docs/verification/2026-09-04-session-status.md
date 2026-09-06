# Windows session and status verification

Date: 2026-09-07 (Windows)

## Automated checks

| Command | Result | Evidence |
| --- | --- | --- |
| `npm test` | PASS | Frontend selectors: 26 passed; Rust library: 113 passed. |
| `npm run build` | PASS | Vite production build completed successfully. |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | PASS | 113 passed, 0 failed. |
| `cargo fmt --manifest-path src-tauri/Cargo.toml --check` | PASS | No formatting differences. |
| `git diff --check` | PASS | No whitespace errors; the three reported working-tree entries are stat-only line-ending metadata with no content diff. |

## Bounded startup smoke

| Command | Result | Observation |
| --- | --- | --- |
| `npm run tauri dev` | STARTED | Vite became ready, Cargo finished the dev build, and Tauri launched `target\\debug\\dynamic-island.exe`. The smoke-created process tree was then stopped by its verified PIDs and no smoke process remained. One non-blocking dead-code warning was printed for `combine_session_scans`; no startup or sandbox error was printed. |

This is startup-only evidence. No interactive GUI observation was performed during this run.

## Acceptance-scenario mapping

| Acceptance scenario | Automated or synthetic coverage | Startup smoke | Manual GUI observation |
| --- | --- | --- | --- |
| Sessions from two projects remain isolated | Synthetic Codex snapshot uses `matching-active` for the active project while retaining unrelated `newer-history` only as history; the active display state is `working`. | App launch reached the executable. | Not performed. |
| Historical sessions appear only in the overview | Selector tests keep the island at no session when no active session exists and include active/history once in overview rows. | App launch reached the executable. | Not performed. |
| A repaired tool failure ends as done | The synthetic Claude `claude-s1` test proves that historical failed-tool data does not prevent a later structured success. The Codex recovered-command test explicitly retains a failed tool event and ends in structured success; status tests cover completed-result highlighting (`done`) before expiry. | App launch reached the executable. | Not performed. |
| An unmatched session displays “无法确认当前会话” | Synthetic ambiguous candidates `candidate-one` and `candidate-two` produce no active session and `session_ambiguous`; the island's unmatched-session copy is present in the frontend. | App launch reached the executable. | Not performed. |
| A stopped process overrides stale approval state | State test confirms `stopped` dominates stale approval; snapshot test retains `stopped` with synthetic `old-history`. | App launch reached the executable. | Not performed. |
| Delayed events from an older turn cannot replace the active turn | Adapter and snapshot tests retain the newer active-turn identity when an older terminal or tool event arrives later. | App launch reached the executable. | Not performed. |
| Incomplete acquisition cannot prove an active session | Production tests cover missing files/directories, partial parses, disappearing or changed indexed records, Hermes command failures, and incomplete evidence even with a matching PID. | App launch reached the executable. | Not performed. |
| Terminal statistics do not recount recent revisits | Tests cover duplicate polls, `A -> B -> A`, save/load revisits, legacy migration, the bounded 128-entry hash window, and serialization without raw identifiers. | App launch reached the executable. | Not performed. |
| Process activity is only a fresh nonterminal fallback | Snapshot tests cover OpenCode/Hermes busy and idle fallback, structured or stopped-state priority, the exact five-second boundary, expired samples, and future timestamps. | App launch reached the executable. | Not performed. |
| Poll freshness and event freshness remain independent | Frontend tests keep a quiet active session eligible after a successful poll, make failed-poll retention stale, and reject event or poll timestamps beyond the five-second clock-skew allowance; Rust tests enforce the same event/session/result upper bound. | App launch reached the executable. | Not performed. |

## Scope and privacy

The verification record contains command outcomes, agent categories, synthetic identifiers, and state/match outcomes only. It excludes prompt bodies, credentials, and full user paths.
