# Windows session and status verification

Date: 2026-09-05 (Windows)

## Automated checks

| Command | Result | Evidence |
| --- | --- | --- |
| `npm test` | PASS | Frontend selectors: 14 passed; Rust library: 82 passed. |
| `npm run build` | PASS | Vite production build completed successfully. |
| `cargo test --manifest-path src-tauri/Cargo.toml --lib` | PASS | 82 passed, 0 failed. |

## Bounded startup smoke

| Command | Result | Observation |
| --- | --- | --- |
| `npm run tauri dev` | STARTED | Within 30 seconds, Vite became ready and Cargo launched `target\\debug\\dynamic-island.exe`. No startup or sandbox error was printed. The process was then intentionally interrupted to keep the smoke test bounded. |

This is startup-only evidence. No interactive GUI observation was performed during this run.

## Acceptance-scenario mapping

| Acceptance scenario | Automated or synthetic coverage | Startup smoke | Manual GUI observation |
| --- | --- | --- | --- |
| Sessions from two projects remain isolated | Synthetic Codex snapshot uses `matching-active` for the active project while retaining unrelated `newer-history` only as history; the active display state is `working`. | App launch reached the executable. | Not performed. |
| Historical sessions appear only in the overview | Selector tests keep the island at no session when no active session exists and include active/history once in overview rows. | App launch reached the executable. | Not performed. |
| A repaired tool failure ends as done | Synthetic Claude `claude-s1` and Codex recovered-command fixtures retain a failed tool event but end in structured success; status tests cover completed-result highlighting (`done`) before expiry. | App launch reached the executable. | Not performed. |
| An unmatched session displays “无法确认当前会话” | Synthetic ambiguous candidates `candidate-one` and `candidate-two` produce no active session and `session_ambiguous`; the island's unmatched-session copy is present in the frontend. | App launch reached the executable. | Not performed. |
| A stopped process overrides stale approval state | State test confirms `stopped` dominates stale approval; snapshot test retains `stopped` with synthetic `old-history`. | App launch reached the executable. | Not performed. |

## Scope and privacy

The verification record contains command outcomes, agent categories, synthetic identifiers, and state/match outcomes only. It excludes prompt bodies, credentials, and full user paths.
