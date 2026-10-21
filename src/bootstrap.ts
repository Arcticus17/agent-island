import { frontendMode, type FrontendMode } from "./frontend-mode";
import { tauriBridge } from "./bridge/tauri";
import { restoreStoredSettings, startSettingsPersistence } from "./settings/persistence";

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
  const svelteRoot = document.getElementById("island-root");
  const legacyIsland = document.getElementById("island");
  if (svelteRoot || legacyIsland) {
    const startFrontend = createFrontendBootstrap({
      mode: frontendMode(),
      svelteRoot,
      legacyIsland,
      loadSvelte: () => import("./app/main"),
      loadLegacy: () => import("./main.js"),
    });

    // 先把磁盘上的设置镜像回 localStorage，再挂载界面（否则会先用默认主题渲染一帧）；
    // 挂载后再开始"变更即落盘"，避免把默认值覆盖掉用户已有的设置。
    void (async () => {
      await restoreStoredSettings(tauriBridge);
      await startFrontend();
      startSettingsPersistence(tauriBridge);
    })();
  }
}
