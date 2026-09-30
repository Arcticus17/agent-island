import { describe, expect, it } from "vitest";
import { DEFAULT_LAYOUTS } from "../src/layout/presets";
import { moveCard, resizeCard, setCardHeight } from "../src/layout/reorder";
import { validateLayout } from "../src/layout/schema";
import { loadLayout, saveLayout } from "../src/layout/persistence";

describe("custom card height", () => {
  it("round-trips height and retains it after reorder without mutating defaults", () => {
    const layout = setCardHeight(DEFAULT_LAYOUTS.monitoring, "log", 333.3);
    let raw: string | null = null;
    const storage = { getItem: () => raw, setItem: (_key: string, value: string) => { raw = value; } };
    expect(saveLayout(storage, moveCard(layout, "log", 0)).ok).toBe(true);
    expect(loadLayout(storage).cards[0].height).toBe(333);
    expect(DEFAULT_LAYOUTS.monitoring.cards.find(c => c.id === "log")?.height).toBeUndefined();
  });
  it("clamps resize and removes override on selecting the same size preset", () => {
    const layout = setCardHeight(DEFAULT_LAYOUTS.monitoring, "log", 900);
    expect(layout.cards.find(c => c.id === "log")?.height).toBe(600);
    expect(setCardHeight(layout, "log", 0).cards.find(c => c.id === "log")?.height).toBe(96);
    expect(resizeCard(layout, "log", "wide").cards.find(c => c.id === "log")?.height).toBeUndefined();
    expect(setCardHeight(layout, "log", NaN)).toBe(layout);
  });
  it("accepts legacy layouts but rejects malformed persisted heights", () => {
    expect(validateLayout(DEFAULT_LAYOUTS.monitoring).ok).toBe(true);
    for (const height of [null, "120", 95, 601, 120.5, NaN]) {
      const layout = structuredClone(DEFAULT_LAYOUTS.monitoring);
      Object.assign(layout.cards[0], { height });
      expect(validateLayout(layout)).toEqual({ ok: false, code: "invalid_card_height" });
    }
  });
});
