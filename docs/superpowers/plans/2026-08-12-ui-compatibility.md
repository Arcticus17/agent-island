# Agent Island UI Compatibility Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make existing Agent Island controls readable, reachable, and stable on Chinese Windows installations across DPI, screen size, monitor, and accessibility variations.

**Architecture:** CSS establishes fluid bounds and responsive layouts for the island and overview. Frontend code clamps the user-selected width to the current viewport and refreshes it on resize. Tauri clamps the main window to monitor work areas whenever restored geometry is applied.

**Tech Stack:** Tauri 2, Rust, Vite 6, native JavaScript, CSS.

## Global Constraints

- Keep Chinese as the default UI language and save all touched source files as UTF-8.
- Preserve existing Agent commands, data contracts, localStorage keys, and visual theme direction.
- Support Windows high-DPI scaling, narrow viewports, light/dark system themes, keyboard navigation, pointer/touch input, and reduced motion.
- Run `npm run build` and `cargo test --manifest-path src-tauri/Cargo.toml --lib` before completion.

---

### Task 1: Restore readable Chinese interface text and valid markup

**Files:**
- Modify: `index.html`
- Modify: `overview.html`
- Modify: `src/main.js`
- Modify: `src/overview.js`

**Interfaces:**
- Consumes: Existing element IDs and command names.
- Produces: Valid UTF-8 labels, titles, ARIA labels, and status strings without changing selectors or behavior.

- [ ] **Step 1: Identify malformed text and attributes**

Run: `Select-String -Path index.html,overview.html,src/main.js,src/overview.js -Pattern '�|title="[^"]*$|aria-label="[^"]*$'`

Expected: Identify encoding/mojibake or malformed attributes for correction.

- [ ] **Step 2: Correct all visible Chinese labels and HTML attributes**

Keep every existing `id`, `data-*` attribute, and command-oriented button behavior. Use Chinese strings such as `加载中…`, `隐私遮罩`, `会话总览`, `打开终端`, and `停止`.

- [ ] **Step 3: Build the frontend**

Run: `npm run build`

Expected: Vite completes without parsing errors.

### Task 2: Make island layout responsive and accessible

**Files:**
- Modify: `src/style.css`
- Modify: `src/main.js`

**Interfaces:**
- Consumes: Existing `themeWidth`, `applyTheme`, `positionIsland`, and island element IDs.
- Produces: `getSafeIslandWidth()` and responsive CSS constraints that preserve current user choices within available bounds.

- [ ] **Step 1: Add a failing DOM-level width expectation**

Add a small Node-free assertion script or testable exported helper that verifies a saved 520px width clamps to a smaller viewport while a 360px minimum remains reachable.

- [ ] **Step 2: Implement viewport-safe width handling**

Implement `getSafeIslandWidth(preferredWidth)` using `window.innerWidth`, a 16px gutter, and a 280px lower fallback. Update the CSS custom property and call it from `applyTheme`, `positionIsland`, and a debounced `resize` listener.

- [ ] **Step 3: Add responsive CSS**

Use `min()`, `max-width`, `max-height`, scrolling body regions, wrapping header/footer actions, and breakpoint rules to collapse the two-column info grid and popover layouts below 360px/420px available width. Ensure interactive targets have at least a 28px practical hit area.

- [ ] **Step 4: Verify with production build**

Run: `npm run build`

Expected: Vite succeeds and emitted CSS contains responsive rules.

### Task 3: Reflow the overview for narrow Windows sizes

**Files:**
- Modify: `src/overview.css`
- Modify: `overview.html`

**Interfaces:**
- Consumes: Existing overview IDs and selected-session behavior.
- Produces: Responsive header, actions, message composer, relay panel, and detail layout.

- [ ] **Step 1: Add narrow-layout rules**

At widths below 520px, wrap header controls and detail actions; at widths below 380px, stack the message composer and relay inputs. Preserve a scrollable session list and chat log using flexbox `min-height: 0`.

- [ ] **Step 2: Improve keyboard and contrast states**

Apply shared `:focus-visible` outlines for buttons, inputs, and selectable rows. Ensure time/text inputs use system color scheme in both themes and keep disabled actions visually distinct.

- [ ] **Step 3: Build the frontend**

Run: `npm run build`

Expected: Vite succeeds.

### Task 4: Keep restored island windows reachable after display changes

**Files:**
- Modify: `src-tauri/src/lib.rs`
- Test: `src-tauri/src/lib.rs` unit-test module

**Interfaces:**
- Consumes: Tauri `WebviewWindow` and monitor APIs plus existing window-position persistence.
- Produces: `clamp_window_position` helper and visible on-screen restored geometry.

- [ ] **Step 1: Write unit tests for geometry clamping**

Add tests asserting that off-screen left/top/right/bottom coordinates move inside a rectangular work area while valid coordinates are unchanged.

- [ ] **Step 2: Implement pure geometry clamp helper**

Use integer logical coordinates and minimum visible margins. Keep the helper independent of Tauri so unit tests do not require a GUI.

- [ ] **Step 3: Apply the helper where saved or startup geometry is restored**

Resolve the window's current or primary monitor work area, clamp saved coordinates, and only then call `set_position`. If no monitor is resolved, preserve the existing behavior.

- [ ] **Step 4: Run Rust tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Expected: All library tests pass.

### Task 5: Full verification and review

**Files:**
- Verify: `index.html`, `overview.html`, `src/main.js`, `src/style.css`, `src/overview.css`, `src-tauri/src/lib.rs`

- [ ] **Step 1: Run production frontend build**

Run: `npm run build`

Expected: exit code 0.

- [ ] **Step 2: Run Rust library tests**

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Expected: exit code 0.

- [ ] **Step 3: Inspect the final diff**

Run: `git diff --check; git diff --stat`

Expected: no whitespace errors; all changes confined to compatibility scope.
