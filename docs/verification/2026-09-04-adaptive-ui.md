# Adaptive UI release verification

Verification date: 2026-09-09. Scope: Windows reliability worktree.

## Confirmed automated evidence

- Final `npm test`: legacy frontend 26/26, unit 107/107, browser interaction 60/60, visual 14/14, Rust 116/116. Exit code 0.
- Visual baseline review: all 14 images inspected. Two themes cover compact idle/working/error, monitoring, approval, long log, and single-column views.
- Negative control: a temporary test-only compact-header height change failed with 2,579 differing pixels. The change was removed, then all 14 visual comparisons passed without updating baselines.
- Missing baseline behavior: `update: "none"` / `--update=none` explicitly rejects missing images. Normal runs never generate approval baselines.
- Svelte/type check: 0 errors and 0 warnings. Frontend and Windows release builds passed.
- Release workflow now requires Node 22, installs Playwright Chromium, and runs type checking and `npm test` before publishing. The remote workflow itself has not been dispatched or verified in this run; Windows runner font differences can require reviewed baseline adjustments.

- Rust library regression: 116 passed, 0 failed; formatting check passed.
- Previous theme gate: unit 107/107, browser 60/60, Svelte check 0 errors and 0 warnings, production frontend build passed.
- Real App browser coverage includes persisted layouts, recovery from invalid configuration, keyboard/card ordering, hidden-card restoration, drag lifecycle and callback failures, refresh deferral, and nonblocking save failures.
- Responsive browser checks cover 520 CSS px double columns and 360/288/240 CSS px single columns. Reduced viewport widths approximate available space at higher scaling; they do not prove native Windows DPI behavior.

## Native acceptance still required

Windows release build succeeded with `npm run tauri build` after an approved network retry for the NSIS tooling download. Both NSIS and MSI bundles were produced. Installation/uninstallation was not performed.

| Bundle | SHA-256 |
| --- | --- |
| `Agent Island_1.8.2_x64-setup.exe` | `3575093B1064576F7C2F07829A473741AC2B38475AE0841EAC4354EC25160563` |
| `Agent Island_1.8.2_x64_en-US.msi` | `62A83EA11195F13E94C70EBE509A1F4577841716FD361C75A8A17DCE1B984003` |

Artifacts are under `src-tauri/target/release/bundle/`; ignored generated binaries are not committed.

| Scenario | Status | Required observation |
| --- | --- | --- |
| Windows 100%, 125%, 150% DPI | Not verified interactively | Text, window bounds, hit targets and popup placement remain correct. |
| Primary and secondary displays | Not verified interactively | Move between screens with different DPI; window stays in the work area. |
| Native WebView drag and saved layout | Browser coverage only | Reorder, close/reopen, verify persisted order; cancel drag and confirm refresh resumes. |
| Tray, shortcuts and terminal jump | Not verified interactively | Commands target the selected live session and remain available. |
| Approval priority and compact/expanded transitions | Browser coverage only | Approval stays actionable during real window resize and scrolling. |
| Installer installation and uninstall | Not verified | Test generated installers on a disposable Windows environment. |

This record does not certify release readiness until the native checks are completed. Generating an installer is separate from installing and testing it. No release is published by this verification work.
