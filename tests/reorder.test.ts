import { describe, expect, it } from "vitest";

import { DEFAULT_LAYOUTS } from "../src/layout/presets";
import { moveCard, resizeCard, setCardVisible } from "../src/layout/reorder";
import { validateLayout, type LayoutConfigV1 } from "../src/layout/schema";

function layout(): LayoutConfigV1 {
  return structuredClone(DEFAULT_LAYOUTS.monitoring);
}

function ids(value: LayoutConfigV1): string[] {
  return value.cards.map(({ id }) => id);
}

describe("layout reorder operations", () => {
  it("moves a card to a clamped target and marks the layout custom", () => {
    const source = layout();

    const first = moveCard(source, "stats", -99);
    const last = moveCard(source, "status", 99);

    expect(ids(first)).toEqual(["stats", "status", "usage", "session", "log"]);
    expect(ids(last)).toEqual(["usage", "session", "log", "stats", "status"]);
    expect(first.preset).toBe("custom");
    expect(last.preset).toBe("custom");
    expect(validateLayout(first).ok).toBe(true);
    expect(validateLayout(last).ok).toBe(true);
    expect(source).toEqual(DEFAULT_LAYOUTS.monitoring);
  });

  it("retains hidden cards and their relative order while visible cards move", () => {
    const source = layout();
    const hiddenUsage = setCardVisible(source, "usage", false);
    const hiddenStats = setCardVisible(hiddenUsage, "stats", false);

    const moved = moveCard(hiddenStats, "log", 0);
    const restoredUsage = setCardVisible(moved, "usage", true);

    expect(ids(moved)).toEqual(["log", "status", "usage", "session", "stats"]);
    expect(moved.cards.filter(({ visible }) => !visible).map(({ id }) => id)).toEqual([
      "usage",
      "stats",
    ]);
    expect(ids(restoredUsage)).toEqual(ids(moved));
    expect(restoredUsage.cards.find(({ id }) => id === "usage")?.visible).toBe(true);
    expect(restoredUsage.cards.find(({ id }) => id === "stats")?.visible).toBe(false);
    expect(validateLayout(restoredUsage).ok).toBe(true);
  });

  it("resizes known cards without mutating the source", () => {
    const source = layout();
    const before = structuredClone(source);

    const resized = resizeCard(source, "usage", "wide");

    expect(resized).not.toBe(source);
    expect(resized.cards.find(({ id }) => id === "usage")?.size).toBe("wide");
    expect(resized.preset).toBe("custom");
    expect(source).toEqual(before);
    expect(validateLayout(resized).ok).toBe(true);
  });

  it("hides and restores optional cards without removing them", () => {
    const source = layout();
    const hidden = setCardVisible(source, "session", false);
    const restored = setCardVisible(hidden, "session", true);

    expect(hidden.cards).toHaveLength(source.cards.length);
    expect(ids(hidden)).toEqual(ids(source));
    expect(hidden.cards.find(({ id }) => id === "session")?.visible).toBe(false);
    expect(restored.cards.find(({ id }) => id === "session")?.visible).toBe(true);
    expect(restored.preset).toBe("custom");
    expect(source).toEqual(DEFAULT_LAYOUTS.monitoring);
  });

  it.each(["status", "log"] as const)("does not hide required %s card", (id) => {
    const source = layout();

    expect(setCardVisible(source, id, false)).toBe(source);
  });

  it("returns the same reference for invalid or no-op requests", () => {
    const source = layout();

    expect(moveCard(source, "unknown", 2)).toBe(source);
    expect(moveCard(source, "status", 0)).toBe(source);
    expect(moveCard(source, "status", Number.NaN)).toBe(source);
    expect(resizeCard(source, "unknown", "wide")).toBe(source);
    expect(resizeCard(source, "usage", "giant")).toBe(source);
    expect(resizeCard(source, "usage", "standard")).toBe(source);
    expect(setCardVisible(source, "unknown", false)).toBe(source);
    expect(setCardVisible(source, "usage", true)).toBe(source);
  });

  it("never mutates frozen input and every changed result remains complete", () => {
    const source = layout();
    source.cards.forEach(Object.freeze);
    Object.freeze(source.cards);
    Object.freeze(source);

    const results = [
      moveCard(source, "log", 0),
      resizeCard(source, "session", "compact"),
      setCardVisible(source, "stats", false),
    ];

    for (const result of results) {
      expect(result.cards).toHaveLength(source.cards.length);
      expect(validateLayout(result).ok).toBe(true);
      expect(result.preset).toBe("custom");
    }
  });
});
