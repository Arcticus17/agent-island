import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

import { createFrontendBootstrap } from "../src/bootstrap";
import { frontendMode } from "../src/frontend-mode";

function container(hidden: boolean): HTMLElement {
  return {
    hidden,
    replaceChildren: vi.fn(),
  } as unknown as HTMLElement;
}

function deferred<T>(): {
  promise: Promise<T>;
  resolve: (value: T) => void;
} {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((done) => {
    resolve = done;
  });
  return { promise, resolve };
}

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

  it("defaults to Svelte after migration", () => {
    localStorage.removeItem("agent-island-ui-legacy");

    expect(frontendMode()).toBe("svelte");
  });

  it("uses legacy only through the reversible rollback flag", () => {
    localStorage.setItem("agent-island-ui-legacy", "1");

    expect(frontendMode()).toBe("legacy");
  });

  it("ignores the retired Svelte opt-in flag", () => {
    localStorage.setItem("agent-island-ui-v2", "0");

    expect(frontendMode()).toBe("svelte");
  });

  it("requires the exact rollback value", () => {
    localStorage.setItem("agent-island-ui-legacy", "0");

    expect(frontendMode()).toBe("svelte");
  });
});

describe("createFrontendBootstrap", () => {
  it("keeps legacy visible until Svelte imports and mounts successfully", async () => {
    const svelteRoot = container(true);
    const legacyIsland = container(false);
    const loaded = deferred<{ mountSvelteIsland: (target: HTMLElement) => void }>();
    const mountSvelteIsland = vi.fn();
    const loadLegacy = vi.fn(async () => undefined);
    const start = createFrontendBootstrap({
      mode: "svelte",
      svelteRoot,
      legacyIsland,
      loadSvelte: () => loaded.promise,
      loadLegacy,
    });

    const startup = start();

    expect(legacyIsland.hidden).toBe(false);
    expect(svelteRoot.hidden).toBe(true);
    loaded.resolve({ mountSvelteIsland });
    await startup;

    expect(mountSvelteIsland).toHaveBeenCalledOnce();
    expect(legacyIsland.hidden).toBe(true);
    expect(svelteRoot.hidden).toBe(false);
    expect(loadLegacy).not.toHaveBeenCalled();
  });

  it.each([
    ["loader", async () => Promise.reject(new Error("blocked_by_csp"))],
    [
      "mount",
      async () => ({
        mountSvelteIsland: () => {
          throw new Error("mount_failed");
        },
      }),
    ],
  ])("restores and initializes legacy after a Svelte %s failure", async (_kind, loadSvelte) => {
    const svelteRoot = container(true);
    const legacyIsland = container(false);
    const loadLegacy = vi.fn(async () => undefined);
    const start = createFrontendBootstrap({
      mode: "svelte",
      svelteRoot,
      legacyIsland,
      loadSvelte,
      loadLegacy,
    });

    await expect(start()).resolves.toBe("legacy");

    expect(legacyIsland.hidden).toBe(false);
    expect(svelteRoot.hidden).toBe(true);
    expect(svelteRoot.replaceChildren).toHaveBeenCalledOnce();
    expect(loadLegacy).toHaveBeenCalledOnce();
  });

  it("deduplicates repeated startup calls", async () => {
    const svelteRoot = container(true);
    const legacyIsland = container(false);
    const mountSvelteIsland = vi.fn();
    const loadSvelte = vi.fn(async () => ({ mountSvelteIsland }));
    const start = createFrontendBootstrap({
      mode: "svelte",
      svelteRoot,
      legacyIsland,
      loadSvelte,
      loadLegacy: vi.fn(async () => undefined),
    });

    const first = start();
    const second = start();
    await Promise.all([first, second]);

    expect(first).toBe(second);
    expect(loadSvelte).toHaveBeenCalledOnce();
    expect(mountSvelteIsland).toHaveBeenCalledOnce();
  });

  it("loads legacy directly without attempting Svelte in rollback mode", async () => {
    const svelteRoot = container(false);
    const legacyIsland = container(true);
    const loadSvelte = vi.fn(async () => ({ mountSvelteIsland: vi.fn() }));
    const loadLegacy = vi.fn(async () => undefined);
    const start = createFrontendBootstrap({
      mode: "legacy",
      svelteRoot,
      legacyIsland,
      loadSvelte,
      loadLegacy,
    });

    await expect(start()).resolves.toBe("legacy");

    expect(legacyIsland.hidden).toBe(false);
    expect(svelteRoot.hidden).toBe(true);
    expect(loadSvelte).not.toHaveBeenCalled();
    expect(loadLegacy).toHaveBeenCalledOnce();
  });

  it("falls back to legacy when the Svelte root is missing", async () => {
    const legacyIsland = container(true);
    const loadSvelte = vi.fn(async () => ({ mountSvelteIsland: vi.fn() }));
    const loadLegacy = vi.fn(async () => undefined);
    const start = createFrontendBootstrap({
      mode: "svelte",
      svelteRoot: null,
      legacyIsland,
      loadSvelte,
      loadLegacy,
    });

    await expect(start()).resolves.toBe("legacy");

    expect(legacyIsland.hidden).toBe(false);
    expect(loadSvelte).not.toHaveBeenCalled();
    expect(loadLegacy).toHaveBeenCalledOnce();
  });
});
