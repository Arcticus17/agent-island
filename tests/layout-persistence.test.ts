import { afterEach, describe, expect, it, vi } from "vitest";

import {
  LAYOUT_BACKUP_KEY,
  LAYOUT_STORAGE_KEY,
  loadLayout,
  resetLayout,
  saveLayout,
  type LayoutStorage,
} from "../src/layout/persistence";
import { DEFAULT_LAYOUTS } from "../src/layout/presets";
import type { LayoutConfigV1 } from "../src/layout/schema";

class MemoryStorage implements LayoutStorage {
  readonly values = new Map<string, string>();
  readonly writes: Array<[string, string]> = [];

  getItem(key: string): string | null {
    return this.values.get(key) ?? null;
  }

  setItem(key: string, value: string): void {
    this.writes.push([key, value]);
    this.values.set(key, value);
  }
}

function customLayout(): LayoutConfigV1 {
  return {
    version: 1,
    preset: "custom",
    cards: [
      { id: "log", size: "wide", visible: true },
      { id: "status", size: "compact", visible: true },
      { id: "stats", size: "standard", visible: false },
      { id: "session", size: "wide", visible: true },
      { id: "usage", size: "compact", visible: false },
    ],
  };
}

function expectOwnedMonitoring(layout: LayoutConfigV1): void {
  expect(layout).toEqual(DEFAULT_LAYOUTS.monitoring);
  expect(layout).not.toBe(DEFAULT_LAYOUTS.monitoring);
  expect(layout.cards).not.toBe(DEFAULT_LAYOUTS.monitoring.cards);
  for (let index = 0; index < layout.cards.length; index += 1) {
    expect(layout.cards[index]).not.toBe(DEFAULT_LAYOUTS.monitoring.cards[index]);
  }
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("layout persistence", () => {
  it("round-trips a valid layout through stable storage and returns owned values", () => {
    const storage = new MemoryStorage();
    const source = customLayout();

    const saved = saveLayout(storage, source);
    const loaded = loadLayout(storage);

    expect(saved).toEqual({ ok: true, value: source });
    expect(storage.values.get(LAYOUT_STORAGE_KEY)).toBe(JSON.stringify(source));
    expect(loaded).toEqual(source);
    expect(loaded).not.toBe(source);
    expect(loaded.cards).not.toBe(source.cards);
    expect(saved.ok && saved.value).not.toBe(source);
  });

  it.each([
    ["corrupt JSON", "{ definitely not json"],
    ["unknown version", JSON.stringify({ ...customLayout(), version: 2 })],
    ["schema-invalid value", JSON.stringify({ ...customLayout(), cards: [] })],
  ])("backs up %s byte-for-byte before returning monitoring", (_name, raw) => {
    const storage = new MemoryStorage();
    storage.values.set(LAYOUT_STORAGE_KEY, raw);

    const loaded = loadLayout(storage);

    expect(storage.writes[0]).toEqual([LAYOUT_BACKUP_KEY, raw]);
    expectOwnedMonitoring(loaded);
  });

  it("returns a fresh monitoring layout when no saved value exists", () => {
    const storage = new MemoryStorage();

    const first = loadLayout(storage);
    first.cards[0].size = "wide";
    const second = loadLayout(storage);

    expectOwnedMonitoring(second);
    expect(second.cards[0].size).toBe(DEFAULT_LAYOUTS.monitoring.cards[0].size);
    expect(storage.writes).toEqual([]);
  });

  it("never throws when storage access or JSON parsing fails during load", () => {
    const getFailure = {
      get getItem() {
        throw new Error("storage_getter_failed");
      },
      setItem() {},
    } as unknown as LayoutStorage;
    expect(() => loadLayout(getFailure)).not.toThrow();
    expectOwnedMonitoring(loadLayout(getFailure));

    const parseStorage = new MemoryStorage();
    parseStorage.values.set(LAYOUT_STORAGE_KEY, "opaque raw bytes");
    vi.spyOn(JSON, "parse").mockImplementation(() => {
      throw new Error("json_parse_failed");
    });
    expect(() => loadLayout(parseStorage)).not.toThrow();
    expectOwnedMonitoring(loadLayout(parseStorage));
  });

  it("never throws when parsed property getters or backup writes fail", () => {
    const raw = "opaque persisted layout";
    const storage = {
      getItem: () => raw,
      get setItem() {
        throw new Error("storage_setter_failed");
      },
    } as unknown as LayoutStorage;
    vi.spyOn(JSON, "parse").mockReturnValue(Object.defineProperty({}, "version", {
      get() {
        throw new Error("json_property_getter_failed");
      },
    }));

    expect(() => loadLayout(storage)).not.toThrow();
    expectOwnedMonitoring(loadLayout(storage));
  });

  it("rejects invalid layouts without writing them", () => {
    const storage = new MemoryStorage();
    const invalid = { ...customLayout(), cards: customLayout().cards.slice(1) };

    expect(saveLayout(storage, invalid)).toEqual({ ok: false, code: "invalid_layout" });
    expect(storage.writes).toEqual([]);
  });

  it("returns a stable non-throwing result when save storage fails", () => {
    const storage: LayoutStorage = {
      getItem: () => null,
      setItem: () => {
        throw new Error("quota_or_policy_failure");
      },
    };

    expect(() => saveLayout(storage, customLayout())).not.toThrow();
    expect(saveLayout(storage, customLayout())).toEqual({
      ok: false,
      code: "layout_storage_write_failed",
    });
  });

  it.each(["minimal", "monitoring", "debugging"] as const)(
    "resets to an owned %s preset and persists an independent value",
    (preset) => {
      const storage = new MemoryStorage();

      const result = resetLayout(storage, preset);

      expect(result).toEqual({ ok: true, value: DEFAULT_LAYOUTS[preset] });
      expect(result.ok).toBe(true);
      if (!result.ok) return;
      expect(result.value).not.toBe(DEFAULT_LAYOUTS[preset]);
      expect(result.value.cards).not.toBe(DEFAULT_LAYOUTS[preset].cards);
      const persisted = JSON.parse(storage.values.get(LAYOUT_STORAGE_KEY)!) as LayoutConfigV1;
      expect(persisted).toEqual(DEFAULT_LAYOUTS[preset]);
      result.value.cards[0].size = "wide";
      expect(persisted).toEqual(DEFAULT_LAYOUTS[preset]);
    },
  );

  it("rejects non-default reset presets and reports reset write failures", () => {
    const storage = new MemoryStorage();
    expect(resetLayout(storage, "custom")).toEqual({
      ok: false,
      code: "invalid_layout_preset",
    });
    expect(resetLayout(storage, "unknown")).toEqual({
      ok: false,
      code: "invalid_layout_preset",
    });
    expect(storage.writes).toEqual([]);

    const failing: LayoutStorage = {
      getItem: () => null,
      setItem: () => {
        throw new Error("write_failed");
      },
    };
    expect(resetLayout(failing, "minimal")).toEqual({
      ok: false,
      code: "layout_storage_write_failed",
    });
  });
});
