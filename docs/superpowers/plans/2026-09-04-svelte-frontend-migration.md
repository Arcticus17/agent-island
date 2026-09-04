# Svelte Frontend Migration Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace direct-DOM frontend code with a typed, state-coordinated Svelte UI without losing any existing island, approval, notification, session, or command behavior.

**Architecture:** Introduce a typed bridge and Svelte application behind a reversible bootstrap flag. Migrate one behavior group at a time against the versioned `AgentViewSnapshot`, keeping the legacy frontend available until parity tests pass.

**Tech Stack:** Svelte, TypeScript, Vite, Tauri 2 JavaScript API, Vitest Browser Mode, Playwright.

**Spec:** `docs/superpowers/specs/2026-09-04-agent-island-reliability-flexible-ui-design.md`

## Global Constraints

- Start only after `2026-09-04-session-status-correctness.md` is complete.
- Keep Tauri 2 and the Rust snapshot contract at `schema_version: 1`.
- `src/bridge` is the only frontend directory allowed to import `@tauri-apps/api`.
- Keep the legacy frontend selectable until Svelte parity and Windows smoke tests pass.
- Polling must not reset selection, scroll, input, approval, or layout-edit state.
- Do not implement draggable layout or final visual polish in this plan.

---

## File Structure

- Create `src/bootstrap.ts`: legacy/Svelte feature switch.
- Create `src/frontend-mode.ts`: pure feature-mode selection.
- Create `src/bridge/types.ts`: snapshot and command types.
- Create `src/bridge/tauri.ts`: production bridge.
- Create `src/bridge/mock.ts`: deterministic browser-test bridge.
- Create `src/stores/agent-store.ts`: snapshot reconciliation and selection.
- Create `src/stores/interaction-store.ts`: input, dragging, approval and deferred snapshot state.
- Create `src/app/App.svelte`: root island composition.
- Create `src/app/main.ts`: Svelte mount.
- Create focused components under `src/components/`.
- Create browser tests under `tests/browser/`.
- Modify `index.html`, `overview.html`, `vite.config.ts`, `package.json`.

### Task 1: Add Svelte, TypeScript, and reversible bootstrap

**Files:**
- Create: `src/bootstrap.ts`
- Create: `src/frontend-mode.ts`
- Create: `src/app/main.ts`
- Create: `src/app/App.svelte`
- Create: `tsconfig.json`
- Create: `svelte.config.js`
- Modify: `index.html`
- Modify: `vite.config.ts`
- Modify: `package.json`

**Interfaces:**
- Produces: `mountSvelteIsland(target: HTMLElement): void` and local-storage flag `agent-island-ui-v2`.

- [ ] **Step 1: Install pinned dependencies into the lockfile**

Run: `npm install -D svelte @sveltejs/vite-plugin-svelte typescript svelte-check vitest @vitest/browser-playwright playwright`

Expected: `package.json` and `package-lock.json` change; install exits successfully.

- [ ] **Step 2: Write a failing bootstrap unit test**

```ts
import { describe, expect, it } from "vitest";
import { frontendMode } from "../src/frontend-mode";

it("defaults to legacy until migration is complete", () => {
  localStorage.removeItem("agent-island-ui-v2");
  expect(frontendMode()).toBe("legacy");
});
```

- [ ] **Step 3: Run and verify failure**

Run: `npx vitest run tests/bootstrap.test.ts`

Expected: failure because `src/frontend-mode.ts` is missing.

- [ ] **Step 4: Implement the switch and minimal mount**

```ts
export function frontendMode(): "legacy" | "svelte" {
  return localStorage.getItem("agent-island-ui-v2") === "1" ? "svelte" : "legacy";
}
```

Keep that pure function in `src/frontend-mode.ts`. The side-effectful `src/bootstrap.ts` imports it:

```ts
import { frontendMode } from "./frontend-mode";

if (frontendMode() === "svelte") {
  import("./app/main");
} else {
  import("./main.js");
}
```

Change `index.html` to load `/src/bootstrap.ts`; configure `@sveltejs/vite-plugin-svelte`; mount `App.svelte` into `#island-root` only in Svelte mode.

Add these package scripts:

```json
"check": "svelte-check --tsconfig ./tsconfig.json",
"test:unit": "vitest run"
```

- [ ] **Step 5: Verify both entry modes**

Run: `npm run check`

Run: `npm run build`

Expected: TypeScript/Svelte checks and both Vite entries pass.

- [ ] **Step 6: Commit**

```bash
git add package.json package-lock.json tsconfig.json svelte.config.js vite.config.ts index.html src/bootstrap.ts src/frontend-mode.ts src/app tests/bootstrap.test.ts
git commit -m "build: add reversible Svelte frontend entry"
```

### Task 2: Create the typed bridge and deterministic mock

**Files:**
- Create: `src/bridge/types.ts`
- Create: `src/bridge/tauri.ts`
- Create: `src/bridge/mock.ts`
- Create: `tests/bridge.test.ts`

**Interfaces:**
- Consumes: Rust `AgentViewSnapshot` schema version 1.
- Produces: `AgentIslandBridge` with `getSnapshot`, `runCommand`, `listenHookEvents`.

- [ ] **Step 1: Write failing contract tests**

```ts
it("rejects an unsupported snapshot schema", async () => {
  const bridge = createMockBridge({ schema_version: 2, generated_at_ms: 1, agents: [] });
  await expect(bridge.getSnapshot()).rejects.toThrow("unsupported_snapshot_schema");
});
```

- [ ] **Step 2: Run and verify failure**

Run: `npx vitest run tests/bridge.test.ts`

Expected: failure because bridge modules do not exist.

- [ ] **Step 3: Implement exact contracts**

```ts
export interface AgentIslandBridge {
  getSnapshot(): Promise<AgentViewSnapshot>;
  runCommand(command: AgentCommand): Promise<CommandResult>;
  listenHookEvents(handler: (event: HookEvent) => void): Promise<() => void>;
}
```

The Tauri implementation calls `get_agent_snapshot` and existing command names. Validate `schema_version === 1` at the boundary. The mock stores a queue of snapshots and command results without importing Tauri.

- [ ] **Step 4: Enforce the import boundary**

Run: `rg -n "@tauri-apps/api" src -g '!bridge/tauri.ts'`

Expected: no matches outside `src/bridge/tauri.ts`.

- [ ] **Step 5: Run checks and commit**

Run: `npm run check`

Run: `npx vitest run tests/bridge.test.ts`

```bash
git add src/bridge tests/bridge.test.ts
git commit -m "feat: add typed Agent Island frontend bridge"
```

### Task 3: Reconcile snapshots without interrupting interaction

**Files:**
- Create: `src/stores/agent-store.ts`
- Create: `src/stores/interaction-store.ts`
- Create: `tests/agent-store.test.ts`

**Interfaces:**
- Consumes: `AgentViewSnapshot`.
- Produces: `createAgentStore(bridge)`, `selectedAgentId`, `selectedSessionId`, `applySnapshot`, `beginInteraction`, `endInteraction`.

- [ ] **Step 1: Write failing store tests**

Test that agent reordering preserves selection by ID, snapshot arrival during input is deferred, and ending interaction applies only the newest deferred snapshot.

```ts
it("does not reset selection when backend order changes", async () => {
  const store = createAgentStore(mockBridge([snapshot("a", "b"), snapshot("b", "a")]));
  await store.refresh();
  store.selectAgent("b");
  await store.refresh();
  expect(store.current().selectedAgentId).toBe("b");
});
```

- [ ] **Step 2: Run and verify failure**

Run: `npx vitest run tests/agent-store.test.ts`

Expected: failure because stores are missing.

- [ ] **Step 3: Implement immutable reconciliation**

Maintain selection IDs separately from the snapshot. Never retain array indexes. While `interactionDepth > 0`, replace `pendingSnapshot` with the newest snapshot; apply it when depth returns to zero. Preserve log follow mode only when the user is already within 24 px of the bottom.

- [ ] **Step 4: Run checks and commit**

Run: `npx vitest run tests/agent-store.test.ts`

Run: `npm run check`

```bash
git add src/stores tests/agent-store.test.ts
git commit -m "feat: coordinate frontend snapshots and interaction state"
```

### Task 4: Migrate compact and expanded read-only views

**Files:**
- Create: `src/components/island/CompactIsland.svelte`
- Create: `src/components/island/ExpandedIsland.svelte`
- Create: `src/components/cards/StatusCard.svelte`
- Create: `src/components/cards/UsageCard.svelte`
- Create: `src/components/cards/SessionCard.svelte`
- Create: `src/components/cards/LogCard.svelte`
- Modify: `src/app/App.svelte`
- Create: `tests/browser/island-readonly.test.ts`

**Interfaces:**
- Consumes: agent and interaction stores.
- Produces: read-only feature parity for status, usage, path, log and Agent switching.

- [ ] **Step 1: Write failing browser tests**

Render with two agents. Assert compact status uses `display_status`, switching agents changes session/log/path together, and a snapshot refresh does not restart unchanged animations.

- [ ] **Step 2: Run and verify failure**

Run: `npx vitest run --browser tests/browser/island-readonly.test.ts`

Expected: component assertions fail because components are missing.

- [ ] **Step 3: Implement components with semantic markup**

Each component receives typed props and emits user intent only. `LogCard` renders normalized record types with stable event IDs; it never parses text. Use CSS classes derived from `display_status`; do not copy theme colors into components.

- [ ] **Step 4: Run browser tests and commit**

Run: `npx vitest run --browser tests/browser/island-readonly.test.ts`

Run: `npm run build`

```bash
git add src/components src/app/App.svelte tests/browser/island-readonly.test.ts
git commit -m "feat: migrate read-only island views to Svelte"
```

### Task 5: Migrate notifications, approvals, and commands

**Files:**
- Create: `src/components/notifications/NotificationStack.svelte`
- Create: `src/components/approvals/ApprovalStack.svelte`
- Create: `src/components/controls/ActionBar.svelte`
- Create: `src/components/controls/SettingsPopover.svelte`
- Modify: `src/app/App.svelte`
- Create: `tests/browser/island-actions.test.ts`

**Interfaces:**
- Consumes: `AgentIslandBridge.runCommand`, hook events, interaction store.
- Produces: feature parity for approve/deny, open, jump, stop, restart, focus/privacy/theme controls.

- [ ] **Step 1: Write failing interaction tests**

Test stop confirmation, failed command feedback and retry, approval priority over ordinary notifications, and snapshot deferral while approval action is pending.

- [ ] **Step 2: Run and verify failure**

Run: `npx vitest run --browser tests/browser/island-actions.test.ts`

Expected: tests fail because action components are missing.

- [ ] **Step 3: Implement command state explicitly**

Each action uses `idle | confirming | pending | succeeded | failed`; disable duplicate submissions while pending; show structured error code and retry only when `CommandResult.retryable` is true. Approval cards remain in a fixed event layer above draggable content.

- [ ] **Step 4: Verify and commit**

Run: `npx vitest run --browser tests/browser/island-actions.test.ts`

Run: `npm run build`

```bash
git add src/components src/app/App.svelte tests/browser/island-actions.test.ts
git commit -m "feat: migrate island actions and event cards to Svelte"
```

### Task 6: Add diagnostics and component-level failure isolation

**Files:**
- Create: `src/components/diagnostics/DiagnosticsPanel.svelte`
- Create: `src/components/common/CardBoundary.svelte`
- Create: `src/bridge/diagnostics.ts`
- Create: `tests/browser/diagnostics.test.ts`
- Modify: `src/app/App.svelte`

**Interfaces:**
- Consumes: structured `DiagnosticView` items and Tauri commands `get_diagnostics`, `export_diagnostics`.
- Produces: local diagnostics display, redacted export, and per-card fallback with reload action.

- [ ] **Step 1: Write failing browser tests**

Inject one failed card and two adapter issues. Assert the failed card alone shows a reload control, the rest of the island remains interactive, and exported JSON contains issue code/session ID but excludes prompt text and full project path.

- [ ] **Step 2: Run and verify failure**

Run: `npx vitest run --browser tests/browser/diagnostics.test.ts`

Expected: failures because diagnostics components are missing.

- [ ] **Step 3: Implement diagnostics boundary and redaction**

Wrap each configurable card in `CardBoundary`; on render failure show card name, stable error code and reload button. `export_diagnostics` returns only app version, OS version, adapter name, synthetic/hash session identifier, event type, issue code, counts and timestamps. Replace every project path with its final directory name and replace message bodies with length/count metadata.

- [ ] **Step 4: Verify and commit**

Run: `npx vitest run --browser tests/browser/diagnostics.test.ts`

Run: `npm run check`

```bash
git add src/components/diagnostics src/components/common src/bridge/diagnostics.ts src/app/App.svelte tests/browser/diagnostics.test.ts
git commit -m "feat: add redacted frontend diagnostics"
```

### Task 7: Migrate overview and make Svelte the default

**Files:**
- Create: `src/windows/overview/OverviewApp.svelte`
- Create: `src/windows/overview/main.ts`
- Modify: `overview.html`
- Modify: `src/bootstrap.ts`
- Create: `tests/browser/overview.test.ts`
- Modify: `README.md`

**Interfaces:**
- Consumes: typed bridge and snapshot history.
- Produces: Svelte overview parity and default Svelte startup with legacy rollback flag.

- [ ] **Step 1: Write failing overview tests**

Verify history is visible only in overview, selecting a row keeps ID across refreshes, send input survives background polling, and status/log/path use the same session.

- [ ] **Step 2: Run and verify failure**

Run: `npx vitest run --browser tests/browser/overview.test.ts`

Expected: failure because the Svelte overview is missing.

- [ ] **Step 3: Implement overview and invert the feature flag**

Load `src/windows/overview/main.ts` from `overview.html`. Change `frontendMode` so Svelte is default and `agent-island-ui-legacy=1` opts into rollback. Document the temporary rollback flag in README.

- [ ] **Step 4: Run full verification**

Run: `npm test`

Run: `npm run check`

Run: `npm run build`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Expected: every command passes.

- [ ] **Step 5: Commit**

```bash
git add src/windows overview.html src/bootstrap.ts tests/browser/overview.test.ts README.md
git commit -m "feat: make the Svelte interface the default"
```
