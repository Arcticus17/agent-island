import { describe, expect, it } from "vitest";

import {
  CARD_IDS,
  REQUIRED_CARD_IDS,
  validateLayout,
  type CardId,
  type CardPlacement,
} from "../src/layout/schema";
import { DEFAULT_LAYOUTS } from "../src/layout/presets";

function card(
  id: CardId,
  size: CardPlacement["size"] = "standard",
  visible = true,
): CardPlacement {
  return { id, size, visible };
}

function validLayout() {
  return {
    version: 1,
    preset: "custom",
    cards: [
      card("status"),
      card("usage", "compact", false),
      card("session", "standard", false),
      card("log", "wide"),
      card("stats", "standard", false),
    ],
  };
}

describe("validateLayout", () => {
  it("accepts a valid custom layout and returns an owned, whitelisted value", () => {
    const input = {
      ...validLayout(),
      ignored: "must not cross the boundary",
      cards: validLayout().cards.map((placement) => ({ ...placement, ignored: true })),
    };

    const result = validateLayout(input);

    expect(result).toEqual({ ok: true, value: validLayout() });
    if (result.ok) {
      expect(result.value).not.toBe(input);
      expect(result.value.cards).not.toBe(input.cards);
    }
  });

  it("rejects duplicate and unknown card IDs", () => {
    expect(validateLayout({
      version: 1,
      preset: "custom",
      cards: [card("status"), card("log"), card("log")],
    }).ok).toBe(false);
    expect(validateLayout({
      version: 1,
      preset: "custom",
      cards: [card("status"), card("log"), { id: "secrets", size: "wide", visible: true }],
    }).ok).toBe(false);
  });

  it("rejects invalid card sizes and visibility values", () => {
    expect(validateLayout({
      version: 1,
      preset: "custom",
      cards: [card("status"), card("log"), { id: "usage", size: "giant", visible: true }],
    }).ok).toBe(false);
    expect(validateLayout({
      version: 1,
      preset: "custom",
      cards: [card("status"), { id: "log", size: "wide", visible: "yes" }],
    }).ok).toBe(false);
  });

  it("rejects invalid versions and preset names", () => {
    expect(validateLayout({ ...validLayout(), version: 2 }).ok).toBe(false);
    expect(validateLayout({ ...validLayout(), version: "1" }).ok).toBe(false);
    expect(validateLayout({ ...validLayout(), preset: "floating" }).ok).toBe(false);
  });

  it("rejects malformed shapes without throwing", () => {
    const malformed: unknown[] = [
      null,
      undefined,
      false,
      "layout",
      [],
      {},
      { version: 1, preset: "custom" },
      { version: 1, preset: "custom", cards: {} },
      { version: 1, preset: "custom", cards: [null] },
      { version: 1, preset: "custom", cards: ["status", "log"] },
    ];

    for (const input of malformed) {
      expect(() => validateLayout(input)).not.toThrow();
      expect(validateLayout(input).ok).toBe(false);
    }

    const hostile = Object.defineProperty({}, "version", {
      get() {
        throw new Error("hostile_layout_getter");
      },
    });
    expect(() => validateLayout(hostile)).not.toThrow();
    expect(validateLayout(hostile).ok).toBe(false);
  });

  it("rejects missing or hidden required status and log cards", () => {
    expect(validateLayout({
      version: 1,
      preset: "custom",
      cards: [card("status"), card("usage")],
    }).ok).toBe(false);
    expect(validateLayout({
      version: 1,
      preset: "custom",
      cards: [card("log"), card("usage")],
    }).ok).toBe(false);

    for (const required of REQUIRED_CARD_IDS) {
      const cards = [card("status"), card("log")].map((placement) =>
        placement.id === required ? { ...placement, visible: false } : placement,
      );
      expect(validateLayout({ version: 1, preset: "custom", cards }).ok).toBe(false);
    }
  });

  it.each(["usage", "session", "stats"] as const)(
    "rejects a missing optional %s card so it remains restorable",
    (missing) => {
      const input = validLayout();
      input.cards = input.cards.filter(({ id }) => id !== missing);

      expect(validateLayout(input)).toEqual({ ok: false, code: "missing_card_id" });
    },
  );

  it("does not mutate valid or invalid input", () => {
    const valid = validLayout();
    const invalid = {
      version: 1,
      preset: "custom",
      cards: [card("status"), card("log"), card("log")],
    };
    const validBefore = structuredClone(valid);
    const invalidBefore = structuredClone(invalid);

    valid.cards.forEach(Object.freeze);
    invalid.cards.forEach(Object.freeze);
    Object.freeze(valid.cards);
    Object.freeze(valid);
    Object.freeze(invalid.cards);
    Object.freeze(invalid);

    validateLayout(valid);
    validateLayout(invalid);

    expect(valid).toEqual(validBefore);
    expect(invalid).toEqual(invalidBefore);
  });
});

describe("DEFAULT_LAYOUTS", () => {
  it("contains three valid presets", () => {
    expect(Object.keys(DEFAULT_LAYOUTS).sort()).toEqual(["debugging", "minimal", "monitoring"]);
    for (const name of ["minimal", "monitoring", "debugging"] as const) {
      expect(validateLayout(DEFAULT_LAYOUTS[name])).toEqual({
        ok: true,
        value: DEFAULT_LAYOUTS[name],
      });
    }
  });

  it("contains every known card exactly once in every preset", () => {
    const expected = [...CARD_IDS].sort();
    for (const layout of Object.values(DEFAULT_LAYOUTS)) {
      expect(layout.cards.map(({ id }) => id).sort()).toEqual(expected);
      expect(new Set(layout.cards.map(({ id }) => id)).size).toBe(CARD_IDS.length);
    }
  });
});
