import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";
import { page } from "vitest/browser";

import LayoutEditor from "../../src/components/layout/LayoutEditor.svelte";
import LayoutEditorHarness from "./fixtures/LayoutEditorHarness.svelte";
import { DEFAULT_LAYOUTS } from "../../src/layout/presets";
import { moveCard } from "../../src/layout/reorder";
import type { LayoutConfigV1 } from "../../src/layout/schema";

const mounted: Array<ReturnType<typeof mount>> = [];

function copy(layout: LayoutConfigV1): LayoutConfigV1 {
  return structuredClone(layout);
}

async function waitFor(check: () => boolean, timeoutMs = 1_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!check()) {
    if (Date.now() >= deadline) throw new Error("browser_test_wait_timeout");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

function renderEditor(overrides: Record<string, unknown> = {}): HTMLElement {
  const target = document.createElement("div");
  target.style.width = "520px";
  document.body.append(target);
  mounted.push(mount(LayoutEditor, {
    target,
    props: { layout: copy(DEFAULT_LAYOUTS.monitoring), ...overrides },
  }));
  return target;
}

function renderHarness(initialLayout: LayoutConfigV1): {
  target: HTMLElement;
  replaceLayout: (layout: LayoutConfigV1) => void;
} {
  const target = document.createElement("div");
  document.body.append(target);
  const component = mount(LayoutEditorHarness, {
    target,
    props: { initialLayout },
  });
  mounted.push(component);
  return { target, replaceLayout: component.replaceLayout };
}

async function enterEditMode(target: HTMLElement): Promise<void> {
  target.querySelector<HTMLButtonElement>('[data-testid="layout-edit-toggle"]')!.click();
  await waitFor(() => target.querySelector('[data-testid="layout-card-controls"]') !== null);
}

function cardIds(target: HTMLElement): string[] {
  return [...target.querySelectorAll<HTMLElement>('[data-testid="layout-card"]')]
    .map(({ dataset }) => dataset.cardId!);
}

function card(target: HTMLElement, id: string): HTMLElement | null {
  return target.querySelector(`[data-testid="layout-card"][data-card-id="${id}"]`);
}

afterEach(async () => {
  while (mounted.length) await unmount(mounted.pop()!);
  document.body.replaceChildren();
  vi.restoreAllMocks();
  await page.viewport(800, 700);
});

describe("layout editor", () => {
  it("supports keyboard height limits and clears custom height through size selection", async () => {
    const changes: LayoutConfigV1[] = [];
    const target = renderEditor({ onLayoutChange: (value: LayoutConfigV1) => changes.push(value) });
    await enterEditMode(target);
    const handle = target.querySelector<HTMLElement>('[data-testid="card-resize-handle"][data-card-id="log"]')!;
    handle.dispatchEvent(new KeyboardEvent("keydown", { key: "End", bubbles: true }));
    await waitFor(() => card(target, "log")?.style.height === "600px");
    expect(changes.at(-1)?.cards.find(c => c.id === "log")?.height).toBe(600);
    handle.dispatchEvent(new KeyboardEvent("keydown", { key: "Home", bubbles: true }));
    await waitFor(() => card(target, "log")?.style.height === "96px");
    const select = target.querySelector<HTMLSelectElement>('[data-testid="card-size"][data-card-id="log"]')!;
    select.dispatchEvent(new Event("change", { bubbles: true }));
    await waitFor(() => card(target, "log")?.style.height === "");
  });

  it("commits pointer resize once and releases cancellation, edit close and unmount", async () => {
    for (const ending of ["pointerup", "pointercancel", "lostpointercapture", "close", "unmount"]) {
      const beginInteraction = vi.fn();
      const endInteraction = vi.fn();
      const changes = vi.fn();
      const target = renderEditor({ beginInteraction, endInteraction, onLayoutChange: changes });
      const component = mounted.at(-1)!;
      await enterEditMode(target);
      const handle = target.querySelector<HTMLElement>('[data-testid="card-resize-handle"][data-card-id="log"]')!;
      vi.spyOn(handle, "setPointerCapture").mockImplementation(() => {});
      vi.spyOn(handle, "hasPointerCapture").mockReturnValue(false);
      handle.dispatchEvent(new PointerEvent("pointerdown", { pointerId: 7, button: 0, clientY: 100, bubbles: true }));
      handle.dispatchEvent(new PointerEvent("pointermove", { pointerId: 7, clientY: 200, bubbles: true }));
      if (ending === "close") {
        target.querySelector<HTMLButtonElement>('[data-testid="layout-edit-toggle"]')!.click();
      } else if (ending === "unmount") {
        await unmount(component);
        mounted.splice(mounted.indexOf(component), 1);
      } else {
        handle.dispatchEvent(new PointerEvent(ending, { pointerId: 7, bubbles: true }));
      }
      await waitFor(() => endInteraction.mock.calls.length === 1);
      expect(beginInteraction).toHaveBeenCalledOnce();
      expect(changes).toHaveBeenCalledTimes(ending === "pointerup" ? 1 : 0);
    }
  });

  it("collapses empty cards only outside edit mode and preserves custom heights", async () => {
    const layout = copy(DEFAULT_LAYOUTS.monitoring);
    layout.cards.find(c => c.id === "log")!.height = 250;
    const target = renderEditor({ layout, emptyCardIds: ["usage", "log"] });
    expect(card(target, "usage")?.classList.contains("empty-card")).toBe(true);
    expect(card(target, "log")?.style.height).toBe("250px");
    expect(card(target, "log")?.classList.contains("empty-card")).toBe(false);
    await enterEditMode(target);
    expect(card(target, "usage")?.classList.contains("empty-card")).toBe(false);
  });

  it("uses an explicit edit mode and gives every button an accessible name", async () => {
    const target = renderEditor();

    expect(target.querySelector('[data-testid="layout-card-controls"]')).toBeNull();
    expect(card(target, "status")?.classList.contains("size-compact")).toBe(true);
    expect(card(target, "log")?.classList.contains("size-wide")).toBe(true);

    await enterEditMode(target);
    expect(target.querySelector('[data-testid="layout-editor"]')?.getAttribute("data-editing")).toBe("true");
    for (const button of target.querySelectorAll<HTMLButtonElement>("button")) {
      expect((button.getAttribute("aria-label") ?? button.textContent ?? "").trim()).not.toBe("");
    }
    expect(target.querySelector('[data-testid="card-hide"][data-card-id="status"]')).toBeNull();
    expect(target.querySelector('[data-testid="card-hide"][data-card-id="log"]')).toBeNull();

    if (import.meta.env.VITE_CAPTURE_UI === "1") {
      await page.screenshot({
        path: "../../.superpowers/sdd/2026-09-04-adaptive-island-ui/task3-layout-editor.png",
      });
    }
  });

  it("moves cards with keyboard buttons through the same pure operation", async () => {
    const changes: LayoutConfigV1[] = [];
    const initial = copy(DEFAULT_LAYOUTS.monitoring);
    const target = renderEditor({
      layout: initial,
      onLayoutChange: (layout: LayoutConfigV1) => changes.push(copy(layout)),
    });
    await enterEditMode(target);

    target.querySelector<HTMLButtonElement>('[data-testid="card-move-up"][data-card-id="stats"]')!.click();
    const expected = moveCard(initial, "stats", 3);
    await waitFor(() => cardIds(target).join(",") === expected.cards.map(({ id }) => id).join(","));

    expect(changes.at(-1)).toEqual(expected);
    expect(changes.at(-1)?.preset).toBe("custom");
  });

  it("moves across hidden cards using visible neighbours and disables true visual edges", async () => {
    const initial = copy(DEFAULT_LAYOUTS.monitoring);
    initial.cards.find(({ id }) => id === "usage")!.visible = false;
    initial.cards.find(({ id }) => id === "stats")!.visible = false;
    const target = renderEditor({ layout: initial });
    await enterEditMode(target);

    expect(target.querySelector<HTMLButtonElement>('[data-testid="card-move-up"][data-card-id="status"]')?.disabled).toBe(true);
    expect(target.querySelector<HTMLButtonElement>('[data-testid="card-move-down"][data-card-id="session"]')?.disabled).toBe(true);
    target.querySelector<HTMLButtonElement>('[data-testid="card-move-down"][data-card-id="status"]')!.click();

    await waitFor(() => cardIds(target).join(",") === "log,status,session");
    expect(target.querySelector<HTMLButtonElement>('[data-testid="card-move-up"][data-card-id="log"]')?.disabled).toBe(true);
    expect(target.querySelector<HTMLButtonElement>('[data-testid="card-move-down"][data-card-id="session"]')?.disabled).toBe(true);
  });

  it("resizes, hides and restores optional cards without losing their slot", async () => {
    const target = renderEditor();
    await enterEditMode(target);

    const size = target.querySelector<HTMLSelectElement>('[data-testid="card-size"][data-card-id="session"]')!;
    size.value = "compact";
    size.dispatchEvent(new Event("change", { bubbles: true }));
    await waitFor(() => card(target, "session")?.classList.contains("size-compact") === true);

    target.querySelector<HTMLButtonElement>('[data-testid="card-hide"][data-card-id="usage"]')!.click();
    await waitFor(() => card(target, "usage") === null);
    const restore = target.querySelector<HTMLButtonElement>('[data-testid="card-restore"][data-card-id="usage"]')!;
    expect(restore.getAttribute("aria-label")).toBe("恢复用量卡片");
    restore.click();
    await waitFor(() => card(target, "usage") !== null);

    expect(cardIds(target)).toEqual(["status", "usage", "log", "session", "stats"]);
    expect(card(target, "session")?.classList.contains("size-compact")).toBe(true);
  });

  it("applies all three presets and resets edits to the selected preset", async () => {
    const presetsBefore = structuredClone(DEFAULT_LAYOUTS);
    const changes: LayoutConfigV1[] = [];
    const target = renderEditor({
      onLayoutChange: (layout: LayoutConfigV1) => changes.push(copy(layout)),
    });
    await enterEditMode(target);

    for (const preset of ["minimal", "monitoring", "debugging"] as const) {
      target.querySelector<HTMLButtonElement>(`[data-testid="layout-preset"][data-preset="${preset}"]`)!.click();
      await waitFor(() => changes.at(-1)?.preset === preset);
      expect(changes.at(-1)).toEqual(DEFAULT_LAYOUTS[preset]);
    }

    target.querySelector<HTMLButtonElement>('[data-testid="card-hide"][data-card-id="usage"]')!.click();
    await waitFor(() => changes.at(-1)?.preset === "custom");
    target.querySelector<HTMLButtonElement>('[data-testid="layout-reset"]')!.click();
    await waitFor(() => changes.at(-1)?.preset === "debugging");
    expect(changes.at(-1)).toEqual(DEFAULT_LAYOUTS.debugging);
    expect(DEFAULT_LAYOUTS).toEqual(presetsBefore);
  });

  it("renders a later external layout prop without remounting", async () => {
    const { target, replaceLayout } = renderHarness(copy(DEFAULT_LAYOUTS.monitoring));
    expect(cardIds(target)).toEqual(["status", "usage", "log", "session", "stats"]);

    replaceLayout(copy(DEFAULT_LAYOUTS.minimal));
    await waitFor(() => cardIds(target).join(",") === "status,log");

    expect(card(target, "status")?.classList.contains("size-compact")).toBe(true);
    expect(card(target, "log")?.classList.contains("size-wide")).toBe(true);
    await enterEditMode(target);
    expect(target.querySelector('[data-testid="layout-reset"]')?.getAttribute("aria-label"))
      .toBe("重置为极简布局");
  });

  it("moves by card ID and target index while balancing drag interaction lifecycle", async () => {
    const beginInteraction = vi.fn();
    const endInteraction = vi.fn();
    const changes: LayoutConfigV1[] = [];
    const initial = copy(DEFAULT_LAYOUTS.monitoring);
    const target = renderEditor({
      layout: initial,
      beginInteraction,
      endInteraction,
      onLayoutChange: (layout: LayoutConfigV1) => changes.push(copy(layout)),
    });
    const beforeEditSource = card(target, "status")!;
    beforeEditSource.dispatchEvent(new DragEvent("dragstart", { bubbles: true }));
    expect(beginInteraction).not.toHaveBeenCalled();
    await enterEditMode(target);

    const source = card(target, "status")!;
    const destination = card(target, "stats")!;
    const transfer = new DataTransfer();
    source.dispatchEvent(new DragEvent("dragstart", { bubbles: true, dataTransfer: transfer }));
    destination.dispatchEvent(new DragEvent("dragover", {
      bubbles: true,
      cancelable: true,
      clientX: 50_000,
      clientY: -50_000,
      dataTransfer: transfer,
    }));
    destination.dispatchEvent(new DragEvent("drop", {
      bubbles: true,
      cancelable: true,
      clientX: 50_000,
      clientY: -50_000,
      dataTransfer: transfer,
    }));
    source.dispatchEvent(new DragEvent("dragend", { bubbles: true, dataTransfer: transfer }));

    const expected = moveCard(initial, "status", 4);
    await waitFor(() => changes.length === 1);
    expect(changes[0]).toEqual(expected);
    expect(cardIds(target)).toEqual(expected.cards.map(({ id }) => id));
    expect(beginInteraction).toHaveBeenCalledOnce();
    expect(endInteraction).toHaveBeenCalledOnce();
  });

  it("rejects a mismatched drag payload and releases the interaction once", async () => {
    const beginInteraction = vi.fn();
    const endInteraction = vi.fn();
    const changes: LayoutConfigV1[] = [];
    const target = renderEditor({
      beginInteraction,
      endInteraction,
      onLayoutChange: (layout: LayoutConfigV1) => changes.push(copy(layout)),
    });
    await enterEditMode(target);

    const source = card(target, "status")!;
    const destination = card(target, "stats")!;
    const transfer = new DataTransfer();
    source.dispatchEvent(new DragEvent("dragstart", { bubbles: true, dataTransfer: transfer }));
    transfer.setData("text/plain", "usage");
    destination.dispatchEvent(new DragEvent("drop", {
      bubbles: true,
      cancelable: true,
      dataTransfer: transfer,
    }));
    source.dispatchEvent(new DragEvent("dragend", { bubbles: true, dataTransfer: transfer }));

    expect(changes).toHaveLength(0);
    expect(beginInteraction).toHaveBeenCalledOnce();
    expect(endInteraction).toHaveBeenCalledOnce();
  });

  it("releases a failed move callback and permits the next drag", async () => {
    const beginInteraction = vi.fn();
    const endInteraction = vi.fn();
    const callbackError = new Error("expected_move_callback_failure");
    const errors: unknown[] = [];
    let shouldThrow = true;
    const target = renderEditor({
      beginInteraction,
      endInteraction,
      onLayoutChange: () => {
        if (shouldThrow) throw callbackError;
      },
    });
    await enterEditMode(target);

    const preventExpectedError = (event: ErrorEvent): void => {
      if (event.error !== callbackError) return;
      errors.push(event.error);
      event.preventDefault();
    };
    window.addEventListener("error", preventExpectedError);
    try {
      const firstTransfer = new DataTransfer();
      card(target, "status")!.dispatchEvent(new DragEvent("dragstart", {
        bubbles: true,
        dataTransfer: firstTransfer,
      }));
      card(target, "stats")!.dispatchEvent(new DragEvent("drop", {
        bubbles: true,
        cancelable: true,
        dataTransfer: firstTransfer,
      }));
      await waitFor(() => endInteraction.mock.calls.length === 1);

      shouldThrow = false;
      const secondTransfer = new DataTransfer();
      card(target, "usage")!.dispatchEvent(new DragEvent("dragstart", {
        bubbles: true,
        dataTransfer: secondTransfer,
      }));
      card(target, "log")!.dispatchEvent(new DragEvent("drop", {
        bubbles: true,
        cancelable: true,
        dataTransfer: secondTransfer,
      }));
      await waitFor(() => endInteraction.mock.calls.length === 2);
    } finally {
      window.removeEventListener("error", preventExpectedError);
    }

    expect(errors).toEqual([callbackError]);
    expect(beginInteraction).toHaveBeenCalledTimes(2);
    expect(endInteraction).toHaveBeenCalledTimes(2);
  });

  it("clears a failed begin callback and permits a later drag", async () => {
    const beginError = new Error("expected_begin_callback_failure");
    const beginInteraction = vi.fn()
      .mockImplementationOnce(() => { throw beginError; })
      .mockImplementation(() => undefined);
    const endInteraction = vi.fn();
    const target = renderEditor({ beginInteraction, endInteraction });
    await enterEditMode(target);

    const source = card(target, "status")!;
    source.dispatchEvent(new DragEvent("dragstart", {
      bubbles: true,
      dataTransfer: new DataTransfer(),
    }));
    expect(source.classList.contains("dragging")).toBe(false);
    expect(endInteraction).not.toHaveBeenCalled();

    source.dispatchEvent(new DragEvent("dragstart", {
      bubbles: true,
      dataTransfer: new DataTransfer(),
    }));
    await waitFor(() => source.classList.contains("dragging"));
    source.dispatchEvent(new DragEvent("dragend", { bubbles: true }));

    expect(beginInteraction).toHaveBeenCalledTimes(2);
    expect(endInteraction).toHaveBeenCalledOnce();
  });

  it("releases an active drag when editing closes, dragcancel fires, or the component unmounts", async () => {
    async function startCase(): Promise<{
      target: HTMLElement;
      endInteraction: ReturnType<typeof vi.fn>;
      component: (typeof mounted)[number];
    }> {
      const endInteraction = vi.fn();
      const target = renderEditor({ endInteraction });
      const component = mounted.at(-1)!;
      await enterEditMode(target);
      card(target, "status")!.dispatchEvent(new DragEvent("dragstart", {
        bubbles: true,
        dataTransfer: new DataTransfer(),
      }));
      return { target, endInteraction, component };
    }

    const closed = await startCase();
    closed.target.querySelector<HTMLButtonElement>('[data-testid="layout-edit-toggle"]')!.click();
    await waitFor(() => closed.endInteraction.mock.calls.length === 1);

    const cancelled = await startCase();
    card(cancelled.target, "status")!.dispatchEvent(new Event("dragcancel", { bubbles: true }));
    expect(cancelled.endInteraction).toHaveBeenCalledOnce();

    const destroyed = await startCase();
    await unmount(destroyed.component);
    mounted.splice(mounted.indexOf(destroyed.component), 1);
    expect(destroyed.endInteraction).toHaveBeenCalledOnce();
  });
});
