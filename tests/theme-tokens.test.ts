// @ts-expect-error The project intentionally omits @types/node; Vitest runs this file in Node.
import { readFileSync, readdirSync } from "node:fs";
// @ts-expect-error The project intentionally omits @types/node; Vitest runs this file in Node.
import { extname, join, relative } from "node:path";
import { describe, expect, it } from "vitest";

declare const process: { cwd(): string };

const root = process.cwd();

const structuralTokens = [
  "--space-1",
  "--space-5",
  "--radius-island",
  "--radius-card",
  "--radius-control",
  "--radius-event",
  "--font-ui",
  "--font-readout",
  "--font-mono",
  "--fs-2xs",
  "--fs-base",
  "--fs-xl",
  "--z-sticky",
  "--z-event",
  "--motion-spring",
  "--motion-out",
  "--duration-fast",
  "--duration-normal",
  "--duration-slow",
] as const;

/** 两套外观都必须完整定义同一组语义令牌：任何一边漏一个就是一块没被主题化的界面。 */
const semanticTokens = [
  "--ground",
  "--ground-raised",
  "--ground-sunken",
  "--sticky",
  "--ink",
  "--ink-log",
  "--ink-2",
  "--ink-3",
  "--rule",
  "--rule-strong",
  "--field",
  "--field-hover",
  "--state-idle",
  "--state-stopped",
  "--state-working",
  "--state-done",
  "--state-error",
  "--state-waiting",
  "--alarm",
  "--alarm-ink",
  "--danger",
  "--danger-ink",
  "--tint-working",
  "--tint-done",
  "--tint-idle",
  "--scrollbar-thumb",
  "--menu-surface",
  "--menu-hover",
  "--elevation-island",
  "--effect-backdrop",
  "--surface-island",
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

function tokenValue(css: string, token: string): string {
  const match = css.match(new RegExp(`${token.replaceAll("-", "\\-")}\\s*:\\s*([^;]+);`));
  if (!match) throw new Error(`missing ${token}`);
  return match[1].trim();
}

function channel(value: number): number {
  const normalized = value / 255;
  return normalized <= 0.03928 ? normalized / 12.92 : ((normalized + 0.055) / 1.055) ** 2.4;
}

function luminance(hex: string): number {
  const value = hex.replace("#", "");
  const [r, g, b] = [0, 2, 4].map((offset) => Number.parseInt(value.slice(offset, offset + 2), 16));
  return 0.2126 * channel(r) + 0.7152 * channel(g) + 0.0722 * channel(b);
}

function contrast(foreground: string, background: string): number {
  const [light, dark] = [luminance(foreground), luminance(background)].sort((a, b) => b - a);
  return (light + 0.05) / (dark + 0.05);
}

describe("Agent Island appearance system", () => {
  it("defines structural, motion and compatibility tokens centrally", () => {
    const css = source("src/themes/tokens.css");
    for (const token of structuralTokens) expect(declared(css, token), token).toBe(true);
    for (const legacy of ["--island-surface", "--island-surface-card", "--island-text-primary", "--island-space-card"]) {
      expect(declared(css, legacy), legacy).toBe(true);
    }
  });

  it.each(["dark", "light"])("fully defines the %s appearance", (theme) => {
    const css = source(`src/themes/${theme}.css`);
    expect(css).toContain(`[data-theme-style="${theme}"]`);
    for (const token of semanticTokens) expect(declared(css, token), `${theme}:${token}`).toBe(true);
  });

  it("keeps alarm fields readable and body text above 4.5:1 in both appearances", () => {
    for (const theme of ["dark", "light"]) {
      const css = source(`src/themes/${theme}.css`);
      // 告警底色是实心亮色，上面压深墨字；普通状态色只出现在字上，不占大块底色。
      expect(contrast(tokenValue(css, "--alarm-ink"), tokenValue(css, "--alarm")), `${theme}:alarm`).toBeGreaterThanOrEqual(4.5);
      expect(contrast(tokenValue(css, "--danger-ink"), tokenValue(css, "--danger")), `${theme}:danger`).toBeGreaterThanOrEqual(4.5);
      for (const token of ["--ink", "--ink-2", "--ink-3", "--ink-log"]) {
        expect(contrast(tokenValue(css, token), tokenValue(css, "--ground")), `${theme}:${token}`).toBeGreaterThanOrEqual(4.5);
      }
      for (const token of ["--state-working", "--state-done", "--state-error", "--state-waiting", "--state-idle"]) {
        expect(contrast(tokenValue(css, token), tokenValue(css, "--ground")), `${theme}:${token}`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it("keeps both appearances free of glass and card shadows", () => {
    for (const theme of ["dark", "light"]) {
      const css = source(`src/themes/${theme}.css`);
      expect(tokenValue(css, "--effect-backdrop")).toBe("none");
      expect(tokenValue(css, "--elevation-island")).toBe("none");
    }
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

  it("loads the token contract and both appearances from the island, and keeps the overview themes working", () => {
    const app = source("src/app/App.svelte");
    for (const file of ["tokens.css", "dark.css", "light.css"]) expect(app).toContain(file);

    const overview = source("src/windows/overview/OverviewApp.svelte");
    for (const file of ["tokens.css", "graphite.css", "pure-black.css"]) expect(overview).toContain(file);
  });

  it("draws the island scrollbar itself so both right corners stay rounded", () => {
    // 原生 scrollbar-color 会让 Chromium 改用标准滚动条，方形轨道会盖住右上/右下圆角。
    const app = source("src/app/App.svelte");
    expect(app).not.toMatch(/\.agent-island\.expanded[^}]*scrollbar-color/s);
    expect(app).toMatch(/\.agent-island\.expanded::-webkit-scrollbar-thumb/);
    expect(app).toMatch(/\.agent-island\.expanded::-webkit-scrollbar-track\s*\{[^}]*margin:\s*22px 0/s);
  });

  it("keeps the native island window free of backdrop blur", () => {
    // 透明原生窗口外溢的模糊层会在胶囊外留下矩形合成边缘。
    const app = source("src/app/App.svelte");
    expect(app).not.toMatch(/\.agent-island\s*\{[^}]*backdrop-filter/s);
    for (const theme of ["dark", "light"]) {
      expect(source(`src/themes/${theme}.css`)).toMatch(/--effect-backdrop\s*:\s*none/);
    }
  });
});
