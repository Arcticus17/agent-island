import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { frontendMode } from "../src/frontend-mode";

describe("frontendMode", () => {
  const values = new Map<string, string>();

  beforeEach(() => {
    values.clear();
    vi.stubGlobal("localStorage", {
      getItem: (key: string) => values.get(key) ?? null,
      removeItem: (key: string) => values.delete(key),
      setItem: (key: string, value: string) => values.set(key, value),
    });
  });

  afterEach(() => {
    vi.unstubAllGlobals();
  });

  it("defaults to legacy until migration is complete", () => {
    localStorage.removeItem("agent-island-ui-v2");

    expect(frontendMode()).toBe("legacy");
  });

  it("enables Svelte only through the reversible opt-in flag", () => {
    localStorage.setItem("agent-island-ui-v2", "1");

    expect(frontendMode()).toBe("svelte");
  });
});
