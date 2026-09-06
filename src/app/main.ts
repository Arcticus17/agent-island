import { mount } from "svelte";

import App from "./App.svelte";

export function mountSvelteIsland(target: HTMLElement): void {
  mount(App, { target });
}
