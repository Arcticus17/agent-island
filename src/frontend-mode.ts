export type FrontendMode = "legacy" | "svelte";

export function frontendMode(): FrontendMode {
  return localStorage.getItem("agent-island-ui-v2") === "1" ? "svelte" : "legacy";
}
