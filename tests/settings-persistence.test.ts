import { describe, expect, it } from "vitest";

import {
  SETTINGS_PREFIX,
  applySettings,
  collectSettings,
  persistStoredSettings,
  restoreStoredSettings,
  settingsFingerprint,
  type SettingsStorage,
} from "../src/settings/persistence";

function fakeStorage(initial: Record<string, string> = {}): SettingsStorage & {
  dump(): Record<string, string>;
} {
  const data = new Map(Object.entries(initial));
  return {
    get length() {
      return data.size;
    },
    key(index: number) {
      return [...data.keys()][index] ?? null;
    },
    getItem(key: string) {
      return data.has(key) ? (data.get(key) as string) : null;
    },
    setItem(key: string, value: string) {
      data.set(key, value);
    },
    removeItem(key: string) {
      data.delete(key);
    },
    dump() {
      return Object.fromEntries(data);
    },
  };
}

describe("settings persistence", () => {
  it("collects only island keys", () => {
    const storage = fakeStorage({
      "agent-island-theme-style-v2": "terminal",
      "agent-island-icon-style": "pixel",
      "unrelated-key": "nope",
    });
    expect(collectSettings(storage)).toEqual({
      "agent-island-theme-style-v2": "terminal",
      "agent-island-icon-style": "pixel",
    });
    expect(Object.keys(collectSettings(storage)).every((key) => key.startsWith(SETTINGS_PREFIX))).toBe(true);
  });

  it("applies stored settings and reports whether anything changed", () => {
    const storage = fakeStorage({ "agent-island-icon-style": "pixel" });
    const changed = applySettings(storage, {
      "agent-island-icon-style": "pixel",
      "agent-island-theme-style-v2": "terminal",
      "unrelated-key": "nope",
      "agent-island-bad": 42,
    });
    expect(changed).toBe(true);
    expect(storage.dump()).toEqual({
      "agent-island-icon-style": "pixel",
      "agent-island-theme-style-v2": "terminal",
    });
    expect(applySettings(storage, { "agent-island-icon-style": "pixel" })).toBe(false);
  });

  it("builds an order-independent fingerprint", () => {
    const a = settingsFingerprint({ "agent-island-a": "1", "agent-island-b": "2" });
    const b = settingsFingerprint({ "agent-island-b": "2", "agent-island-a": "1" });
    const c = settingsFingerprint({ "agent-island-b": "3", "agent-island-a": "1" });
    expect(a).toBe(b);
    expect(a).not.toBe(c);
  });

  it("round-trips through JSON like the disk store does", () => {
    const source = fakeStorage({
      "agent-island-theme-style-v2": "terminal",
      "agent-island-layout-v1": '{"version":1,"preset":"monitoring"}',
    });
    const payload = JSON.stringify(collectSettings(source));
    const target = fakeStorage({ "agent-island-theme-style-v2": "graphite" });
    expect(applySettings(target, JSON.parse(payload))).toBe(true);
    expect(target.dump()).toEqual(source.dump());
  });
});

describe("settings disk mirror", () => {
  it("migrates existing localStorage into the file on first run", async () => {
    const storage = fakeStorage({ "agent-island-icon-style": "pixel" });
    const saved: string[] = [];
    const bridge = {
      async loadAppSettings() {
        return null;
      },
      async saveAppSettings(settings: string) {
        saved.push(settings);
      },
    };
    await expect(restoreStoredSettings(bridge, storage)).resolves.toBe(false);
    expect(saved).toHaveLength(1);
    expect(JSON.parse(saved[0])).toEqual({ "agent-island-icon-style": "pixel" });
  });

  it("restores disk settings into localStorage", async () => {
    const storage = fakeStorage({ "agent-island-theme-style-v2": "graphite" });
    const bridge = {
      async loadAppSettings() {
        return JSON.stringify({ "agent-island-theme-style-v2": "terminal" });
      },
      async saveAppSettings() {},
    };
    await expect(restoreStoredSettings(bridge, storage)).resolves.toBe(true);
    expect(storage.getItem("agent-island-theme-style-v2")).toBe("terminal");
    // 第二次已经是同一个值，不再算"改过"
    await expect(restoreStoredSettings(bridge, storage)).resolves.toBe(false);
  });

  it("survives corrupt files and broken bridges", async () => {
    const storage = fakeStorage({ "agent-island-icon-style": "dot" });
    await expect(
      restoreStoredSettings({ async loadAppSettings() { return "{oops"; }, async saveAppSettings() {} }, storage),
    ).resolves.toBe(false);
    await expect(
      restoreStoredSettings({ async loadAppSettings() { throw new Error("ipc down"); } }, storage),
    ).resolves.toBe(false);
    expect(storage.getItem("agent-island-icon-style")).toBe("dot");
  });

  it("writes the current snapshot on demand", async () => {
    const storage = fakeStorage({ "agent-island-a": "1", other: "2" });
    let written = "";
    await persistStoredSettings(
      { async saveAppSettings(settings: string) { written = settings; } },
      storage,
    );
    expect(JSON.parse(written)).toEqual({ "agent-island-a": "1" });
  });
});
