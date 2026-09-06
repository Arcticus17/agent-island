import { frontendMode, type FrontendMode } from "./frontend-mode";

interface SvelteIslandModule {
  mountSvelteIsland(target: HTMLElement): void;
}

interface FrontendBootstrapOptions {
  mode: FrontendMode;
  svelteRoot: HTMLElement | null;
  legacyIsland: HTMLElement | null;
  loadSvelte: () => Promise<SvelteIslandModule>;
  loadLegacy: () => Promise<unknown>;
}

export function createFrontendBootstrap({
  mode,
  svelteRoot,
  legacyIsland,
  loadSvelte,
  loadLegacy,
}: FrontendBootstrapOptions): () => Promise<FrontendMode> {
  let startup: Promise<FrontendMode> | undefined;

  const run = async (): Promise<FrontendMode> => {
    if (legacyIsland) {
      legacyIsland.hidden = false;
    }
    if (svelteRoot) {
      svelteRoot.hidden = true;
    }

    if (mode === "svelte" && svelteRoot) {
      try {
        const { mountSvelteIsland } = await loadSvelte();
        mountSvelteIsland(svelteRoot);
        if (legacyIsland) {
          legacyIsland.hidden = true;
        }
        svelteRoot.hidden = false;
        return "svelte";
      } catch {
        svelteRoot.replaceChildren();
        svelteRoot.hidden = true;
        if (legacyIsland) {
          legacyIsland.hidden = false;
        }
      }
    }

    await loadLegacy();
    return "legacy";
  };

  return () => {
    startup ??= run();
    return startup;
  };
}

if (typeof document !== "undefined") {
  const startFrontend = createFrontendBootstrap({
    mode: frontendMode(),
    svelteRoot: document.getElementById("island-root"),
    legacyIsland: document.getElementById("island"),
    loadSvelte: () => import("./app/main"),
    loadLegacy: () => import("./main.js"),
  });

  void startFrontend();
}
