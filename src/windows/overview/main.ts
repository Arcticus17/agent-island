import { createFrontendBootstrap } from "../../bootstrap";
import { frontendMode } from "../../frontend-mode";

interface OverviewModule {
  mountSvelteIsland(target: HTMLElement): void;
}

async function loadOverview(): Promise<OverviewModule> {
  const [{ mount }, { default: OverviewApp }, { tauriBridge }] = await Promise.all([
    import("svelte"),
    import("./OverviewApp.svelte"),
    import("../../bridge/tauri"),
  ]);
  return {
    mountSvelteIsland(target) {
      mount(OverviewApp, { target, props: { bridge: tauriBridge } });
    },
  };
}

const svelteRoot = document.getElementById("overview-root");
const legacyOverview = document.getElementById("overview-legacy");

if (svelteRoot || legacyOverview) {
  const startOverview = createFrontendBootstrap({
    mode: frontendMode(),
    svelteRoot,
    legacyIsland: legacyOverview,
    loadSvelte: loadOverview,
    loadLegacy: () => import("../../overview.js"),
  });
  void startOverview();
}
