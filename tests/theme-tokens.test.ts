// @ts-expect-error The project intentionally omits @types/node; Vitest runs this file in Node.
import { readFileSync, readdirSync } from "node:fs";
// @ts-expect-error The project intentionally omits @types/node; Vitest runs this file in Node.
import { extname, join, relative } from "node:path";
import { describe, expect, it } from "vitest";

declare const process: { cwd(): string };

const root = process.cwd();

const structuralTokens = [
  "--space-card",
  "--space-section",
  "--space-control",
  "--radius-island",
  "--radius-card",
  "--radius-control",
  "--radius-event",
  "--z-sticky",
  "--z-event",
  "--motion-spring",
  "--duration-fast",
  "--duration-normal",
  "--duration-slow",
] as const;

const themedTokens = [
  "--surface-island",
  "--surface-card",
  "--surface-raised",
  "--surface-log",
  "--surface-event",
  "--surface-notification",
  "--surface-sticky",
  "--text-primary",
  "--text-secondary",
  "--text-tertiary",
  "--text-log",
  "--text-error",
  "--border-subtle",
  "--border-event",
  "--border-log-divider",
  "--accent-primary",
  "--status-idle",
  "--status-working",
  "--status-error",
  "--status-done",
  "--status-waiting",
  "--status-stopped",
  "--refresh-error-text",
  "--refresh-error-surface",
  "--control-surface",
  "--control-hover",
  "--scrollbar-thumb",
  "--scrollbar-track",
  "--elevation-island",
  "--elevation-event",
  "--effect-backdrop",
  "--island-background",
] as const;

function source(path: string): string {
  return readFileSync(join(root, path), "utf8") as string;
}

function filesBelow(directory: string): string[] {
  const entries = readdirSync(directory, { withFileTypes: true }) as Array<{
    name: string;
    isDirectory(): boolean;
  }>;
  return entries.flatMap((entry) => {
    const path = join(directory, entry.name);
    return entry.isDirectory() ? filesBelow(path) : [path];
  });
}

function declared(css: string, token: string): boolean {
  return new RegExp(`${token.replaceAll("-", "\\-")}\\s*:`).test(css);
}

describe("Agent Island semantic themes", () => {
  it("defines structural, motion and compatibility tokens centrally", () => {
    const css = source("src/themes/tokens.css");
    for (const token of structuralTokens) expect(declared(css, token), token).toBe(true);
    for (const legacy of ["--island-surface", "--island-surface-card", "--island-text-primary", "--island-space-card"]) {
      expect(declared(css, legacy), legacy).toBe(true);
    }
  });

  it.each(["graphite", "pure-black"])("fully defines the %s theme", (theme) => {
    const css = source(`src/themes/${theme}.css`);
    expect(css).toContain(`[data-theme-style="${theme}"]`);
    for (const token of themedTokens) expect(declared(css, token), `${theme}:${token}`).toBe(true);
  });

  it("keeps pure black solid and independent of backdrop glass", () => {
    const css = source("src/themes/pure-black.css");
    expect(css).toMatch(/--surface-island\s*:\s*#[0-9a-f]{6}\s*;/i);
    expect(css).toMatch(/--surface-card\s*:\s*#[0-9a-f]{6}\s*;/i);
    expect(css).toMatch(/--effect-backdrop\s*:\s*none\s*;/i);
  });

  it("keeps theme color literals out of Svelte component styles", () => {
    const violations = filesBelow(join(root, "src"))
      .filter((path) => extname(path) === ".svelte")
      .flatMap((path) => [...source(relative(root, path)).matchAll(/<style(?:\s[^>]*)?>([\s\S]*?)<\/style>/gi)]
        .filter((match) => /#[0-9a-f]{3,8}\b|rgba?\(|hsla?\(/i.test(match[1]))
        .map(() => relative(root, path)));
    expect(violations).toEqual([]);
  });

  it("globally reduces duration and disables repeating motion", () => {
    const css = source("src/themes/tokens.css");
    expect(css).toMatch(/@media\s*\(prefers-reduced-motion:\s*reduce\)/);
    expect(css).toMatch(/--duration-fast\s*:\s*1ms/);
    expect(css).toMatch(/animation-duration\s*:\s*1ms\s*!important/);
    expect(css).toMatch(/animation-iteration-count\s*:\s*1\s*!important/);
  });

  it("loads the token contract and both theme definitions from the App", () => {
    const app = source("src/app/App.svelte");
    for (const file of ["tokens.css", "graphite.css", "pure-black.css"]) expect(app).toContain(file);
  });
});
