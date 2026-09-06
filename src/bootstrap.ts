import { frontendMode } from "./frontend-mode";

const svelteRoot = document.getElementById("island-root");
const legacyIsland = document.getElementById("island");

if (frontendMode() === "svelte") {
  if (!(svelteRoot instanceof HTMLElement)) {
    throw new Error("missing_island_root");
  }

  legacyIsland?.setAttribute("hidden", "");
  svelteRoot.removeAttribute("hidden");
  void import("./app/main").then(({ mountSvelteIsland }) => {
    mountSvelteIsland(svelteRoot);
  });
} else {
  svelteRoot?.setAttribute("hidden", "");
  void import("./main.js");
}
