# Adaptive Island UI Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Deliver the approved adaptive modular island with constrained card rearrangement, layout presets, resilient persistence, and the graphite-glass visual system.

**Architecture:** Keep identity, urgent events, and core actions fixed while rendering configurable content cards in a responsive CSS grid. Store a versioned logical layout rather than pixel coordinates, and express all visual choices through theme tokens.

**Tech Stack:** Svelte, TypeScript, CSS Grid, HTML drag events plus keyboard reordering, Vitest Browser Mode, Playwright screenshots, Tauri 2.

**Spec:** `docs/superpowers/specs/2026-09-04-agent-island-reliability-flexible-ui-design.md`

## Global Constraints

- Start only after `2026-09-04-svelte-frontend-migration.md` is complete.
- Use the approved B layout and B graphite-glass visual direction.
- Identity/status, approvals/urgent notifications, and core actions are fixed regions.
- Cards may reorder, hide, restore, and use compact/standard/wide sizes; they may not overlap or use saved pixel coordinates.
- Small windows collapse to one column and preserve access to every critical action.
- Respect `prefers-reduced-motion` and never replay entry animation for unchanged snapshot data.
- Keep a pure-black theme preset.

---

## File Structure

- Create `src/layout/schema.ts`: versioned layout model and validation.
- Create `src/layout/reorder.ts`: pure reorder/resize/hide operations.
- Create `src/layout/persistence.ts`: load, migrate, backup and reset.
- Create `src/layout/presets.ts`: minimal, monitoring and debugging layouts.
- Create `src/components/layout/CardGrid.svelte`: responsive card host.
- Create `src/components/layout/LayoutEditor.svelte`: edit mode and controls.
- Create `src/themes/tokens.css`: shared semantic design tokens.
- Create `src/themes/graphite.css`: default theme.
- Create `src/themes/pure-black.css`: alternate theme.
- Create `tests/layout-schema.test.ts` and browser/screenshot suites.
- Modify `src/app/App.svelte` and component styles to consume tokens.

### Task 1: Define and validate the versioned layout schema

**Files:**
- Create: `src/layout/schema.ts`
- Create: `src/layout/presets.ts`
- Create: `tests/layout-schema.test.ts`

**Interfaces:**
- Produces: `LayoutConfigV1`, `CardPlacement`, `CardId`, `CardSize`, `validateLayout`, `DEFAULT_LAYOUTS`.

- [ ] **Step 1: Write failing schema tests**

```ts
it("rejects duplicate cards and restores required cards", () => {
  const result = validateLayout({ version: 1, cards: [card("log"), card("log")] });
  expect(result.ok).toBe(false);
});

it("contains three valid presets", () => {
  for (const name of ["minimal", "monitoring", "debugging"])
    expect(validateLayout(DEFAULT_LAYOUTS[name]).ok).toBe(true);
});
```

- [ ] **Step 2: Run and verify failure**

Run: `npx vitest run tests/layout-schema.test.ts`

Expected: failure because layout modules are missing.

- [ ] **Step 3: Implement the schema**

```ts
export type CardId = "status" | "usage" | "session" | "log" | "stats";
export type CardSize = "compact" | "standard" | "wide";
export interface CardPlacement { id: CardId; size: CardSize; visible: boolean; }
export interface LayoutConfigV1 { version: 1; preset: "custom" | "minimal" | "monitoring" | "debugging"; cards: CardPlacement[]; }
```

Validation rejects unknown IDs, duplicates, invalid sizes and missing required `status`/`log` entries. Presets contain each known card exactly once.

- [ ] **Step 4: Run tests and commit**

Run: `npx vitest run tests/layout-schema.test.ts`

```bash
git add src/layout/schema.ts src/layout/presets.ts tests/layout-schema.test.ts
git commit -m "feat: define versioned island layouts"
```

### Task 2: Persist, migrate, back up, and reset layouts

**Files:**
- Create: `src/layout/persistence.ts`
- Create: `tests/layout-persistence.test.ts`

**Interfaces:**
- Consumes: `LayoutConfigV1`, `validateLayout`.
- Produces: `loadLayout(storage)`, `saveLayout(storage, layout)`, `resetLayout(storage, preset)`.

- [ ] **Step 1: Write failing persistence tests**

Test valid round-trip, corrupt JSON fallback, unknown version fallback, and backup under `agent-island-layout-backup` before replacement.

- [ ] **Step 2: Run and verify failure**

Run: `npx vitest run tests/layout-persistence.test.ts`

Expected: failure because persistence functions are missing.

- [ ] **Step 3: Implement safe persistence**

Use keys `agent-island-layout-v1` and `agent-island-layout-backup`. On invalid input, copy the raw value to backup, return the monitoring preset, and never throw during application startup.

- [ ] **Step 4: Run tests and commit**

Run: `npx vitest run tests/layout-persistence.test.ts`

```bash
git add src/layout/persistence.ts tests/layout-persistence.test.ts
git commit -m "feat: persist and recover island layouts"
```

### Task 3: Implement constrained reordering and keyboard controls

**Files:**
- Create: `src/layout/reorder.ts`
- Create: `src/components/layout/CardGrid.svelte`
- Create: `src/components/layout/LayoutEditor.svelte`
- Create: `tests/reorder.test.ts`
- Create: `tests/browser/layout-editor.test.ts`

**Interfaces:**
- Produces: `moveCard(layout, cardId, targetIndex)`, `resizeCard`, `setCardVisible`, edit-mode component events.

- [ ] **Step 1: Write failing pure-operation tests**

Assert reorder is stable, hidden cards retain relative order, invalid indices clamp safely, and required cards cannot be hidden.

- [ ] **Step 2: Run and verify failure**

Run: `npx vitest run tests/reorder.test.ts`

Expected: failure because reorder operations are missing.

- [ ] **Step 3: Implement pure operations**

Return new layout objects without mutating input. `setCardVisible` returns the original layout when asked to hide a required card. `moveCard` clamps the target to `0..cards.length-1`.

- [ ] **Step 4: Write and run failing browser interactions**

Test entering edit mode, dragging a card, moving it with keyboard buttons, changing size, hiding/restoring optional cards, selecting a preset and resetting default.

Run: `npx vitest run --browser tests/browser/layout-editor.test.ts`

Expected: failures because editor components are not implemented.

- [ ] **Step 5: Implement grid and editor**

Use CSS Grid classes `size-compact`, `size-standard`, and `size-wide`. Native drag events emit card IDs and indices; keyboard controls call the same pure `moveCard` function. Suspend visual snapshot application through `beginInteraction/endInteraction` while dragging.

- [ ] **Step 6: Verify and commit**

Run: `npx vitest run tests/reorder.test.ts`

Run: `npx vitest run --browser tests/browser/layout-editor.test.ts`

```bash
git add src/layout/reorder.ts src/components/layout tests/reorder.test.ts tests/browser/layout-editor.test.ts
git commit -m "feat: add constrained card layout editing"
```

### Task 4: Add responsive degradation rules

**Files:**
- Modify: `src/components/layout/CardGrid.svelte`
- Modify: `src/app/App.svelte`
- Create: `tests/browser/responsive-layout.test.ts`

**Interfaces:**
- Consumes: logical card sizes.
- Produces: two-column wide layout and single-column compact layout without changing saved config.

- [ ] **Step 1: Write failing viewport tests**

At 520 px assert two-column placement and wide cards spanning both columns. At 360 px and 125%/150% scale simulations assert one column, no horizontal overflow, visible approval actions and visible core action menu.

- [ ] **Step 2: Run and verify failure**

Run: `npx vitest run --browser tests/browser/responsive-layout.test.ts`

Expected: viewport assertions fail.

- [ ] **Step 3: Implement responsive CSS**

Use `grid-template-columns: repeat(2, minmax(0, 1fr))` above the compact breakpoint and one column below it. Map every size to one column in compact mode without writing that temporary layout back to storage. Use `min-width: 0`, wrapping action labels and bounded log height to prevent overflow.

- [ ] **Step 4: Verify and commit**

Run: `npx vitest run --browser tests/browser/responsive-layout.test.ts`

```bash
git add src/components/layout/CardGrid.svelte src/app/App.svelte tests/browser/responsive-layout.test.ts
git commit -m "feat: adapt island layout to compact displays"
```

### Task 5: Implement graphite-glass design tokens and pure-black preset

**Files:**
- Create: `src/themes/tokens.css`
- Create: `src/themes/graphite.css`
- Create: `src/themes/pure-black.css`
- Modify: `src/app/App.svelte`
- Modify: all Svelte component styles that contain literal theme colors.
- Create: `tests/theme-tokens.test.ts`

**Interfaces:**
- Produces: semantic tokens such as `--surface-island`, `--surface-card`, `--text-primary`, `--status-working`, `--motion-spring`, and theme selector `data-theme-style`.

- [ ] **Step 1: Write failing token tests**

Read both theme files and assert every required semantic token is present. Scan `.svelte` files and fail on hexadecimal/rgb color literals outside theme files.

- [ ] **Step 2: Run and verify failure**

Run: `npx vitest run tests/theme-tokens.test.ts`

Expected: failure because token files are missing and components contain literal colors.

- [ ] **Step 3: Implement tokens**

Define structure, typography, spacing, radius, elevation and motion in `tokens.css`; define graphite surfaces and restrained blue accents in `graphite.css`; define solid near-black surfaces and neutral controls in `pure-black.css`. Status colors remain semantic and meet readable contrast against both surfaces.

- [ ] **Step 4: Replace component literals and verify reduced motion**

Use only semantic variables in components. Under `@media (prefers-reduced-motion: reduce)`, set motion durations to `1ms`, remove repeating breathing/ping animation, and retain state changes through color/text.

- [ ] **Step 5: Run tests and commit**

Run: `npx vitest run tests/theme-tokens.test.ts`

Run: `npm run check`

Run: `npm run build`

```bash
git add src/themes src/app src/components tests/theme-tokens.test.ts
git commit -m "feat: add graphite-glass island theme"
```

### Task 6: Add visual regression and Windows release evidence

**Files:**
- Create: `tests/visual/island.visual.test.ts`
- Create: `tests/visual/__screenshots__/` approved baselines.
- Create: `docs/verification/2026-09-04-adaptive-ui.md`
- Modify: `package.json`

**Interfaces:**
- Consumes: completed layout and themes.
- Produces: repeatable screenshots and release-readiness evidence.

- [ ] **Step 1: Add visual test scripts**

Add `test:visual` for Playwright screenshot tests and include non-updating visual comparison in the main CI test command.

- [ ] **Step 2: Capture deterministic baselines**

Use the mock bridge and fixed clock. Capture compact idle/working/error, expanded monitoring preset, approval card, long log, 360 px single-column, graphite and pure-black themes. Disable caret and unrelated animation during capture.

- [ ] **Step 3: Verify visual tests detect change**

Temporarily alter one test-only expected class, run `npm run test:visual`, and confirm a screenshot mismatch. Revert the test-only alteration with `apply_patch`, rerun, and confirm pass.

- [ ] **Step 4: Run full automated verification**

Run: `npm test`

Run: `npm run check`

Run: `npm run build`

Run: `cargo test --manifest-path src-tauri/Cargo.toml --lib`

Expected: every command passes.

- [ ] **Step 5: Run Windows manual checks and record results**

Run: `npm run tauri dev` and verify 100%, 125%, 150% DPI; primary and secondary monitors; compact/expanded transitions; drag persistence; preset reset; notification/approval priority; tray, shortcuts and terminal jump. Record pass/fail without private logs or paths.

- [ ] **Step 6: Commit**

```bash
git add package.json package-lock.json tests/visual docs/verification/2026-09-04-adaptive-ui.md
git commit -m "test: add adaptive island visual regression"
```
