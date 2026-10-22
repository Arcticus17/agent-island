// @ts-expect-error 项目故意不装 @types/node；这个文件由 Vitest 在 Node 里跑
import { readFileSync } from "node:fs";
// @ts-expect-error 同上
import { join } from "node:path";
import { describe, expect, it } from "vitest";

declare const process: { cwd(): string };
const root = process.cwd();

const commandsSource = readFileSync(join(root, "src/palette/commands.ts"), "utf8");
const appSource = readFileSync(join(root, "src/app/App.svelte"), "utf8");

/** 从 `{ id: "action:xxx", … }` 列表里抠出所有固定动作 id。 */
function declaredActionIds(source: string): string[] {
  return [...source.matchAll(/\{\s*id:\s*"(action:[a-z-]+)"/g)].map((match) => match[1]);
}

/** 从 `const paletteHandlers: Record<PaletteActionId, …> = { … }` 里抠出已实现的 id。 */
function handledActionIds(source: string): string[] {
  const start = source.indexOf("const paletteHandlers");
  expect(start, "App 里应该有 paletteHandlers 映射").toBeGreaterThan(-1);
  const block = source.slice(start, start + 1200);
  return [...block.matchAll(/"(action:[a-z-]+)":/g)].map((match) => match[1]);
}

describe("palette action contract", () => {
  it("implements every declared action exactly once", () => {
    const declared = declaredActionIds(commandsSource);
    const handled = handledActionIds(appSource);
    expect(declared.length).toBeGreaterThan(5);
    expect(new Set(declared).size, "动作 id 不能重复").toBe(declared.length);
    // 这条断言就是第七轮的教训：列表里有、switch 里没有 = 点了没反应
    expect([...handled].sort()).toEqual([...declared].sort());
  });

  it("keeps the declared action ids stable", () => {
    expect(declaredActionIds(commandsSource)).toEqual([
      "action:terminal",
      "action:refresh",
      "action:privacy",
      "action:focus",
      "action:pin",
      "action:onboarding",
      "action:hide",
    ]);
  });
});

