export type FrontendMode = "legacy" | "svelte";

export function frontendMode(): FrontendMode {
  return localStorage.getItem("agent-island-ui-legacy") === "1" ? "legacy" : "svelte";
}
