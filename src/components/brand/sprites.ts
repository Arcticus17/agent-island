/**
 * 像素吉祥物精灵：每个 Agent 一张 8×8 的单色网格。
 * `#` 为点亮格，其余为空。用单色（currentColor）是为了让同一张精灵
 * 跟随状态色变化，和 Mac 参考片里的"同一只螃蟹换色"保持一致。
 */
export type SpriteKey = "claude" | "codex" | "opencode" | "hermes" | "generic";

export const PIXEL_GRID = 8;

const SPRITES: Record<SpriteKey, readonly string[]> = {
  // 小螃蟹：两根眼柄 + 宽身体 + 两只脚，对应 Claude Code 的吉祥物剪影
  claude: [
    "........",
    "..#..#..",
    "########",
    "##.##.##",
    "########",
    ".######.",
    ".#.##.#.",
    "........",
  ],
  // 机器人头：天线 + 方脸 + 两只眼洞
  codex: [
    "........",
    "...##...",
    "..####..",
    ".######.",
    "##.##.##",
    "########",
    ".#....#.",
    "........",
  ],
  // 宝石/盾形
  opencode: [
    "........",
    "...##...",
    "..####..",
    ".######.",
    "########",
    ".######.",
    "..####..",
    "...##...",
  ],
  // 闪电
  hermes: [
    "....###.",
    "...###..",
    "..####..",
    ".######.",
    "...###..",
    "..###...",
    ".###....",
    "........",
  ],
  // 兜底：一只看不出是谁的小生物，也带眼洞
  generic: [
    "........",
    "..####..",
    ".######.",
    "##.##.##",
    "########",
    ".######.",
    "..####..",
    "........",
  ],
};

/** 按 Agent 名称（或 id）匹配精灵；匹配不到时用 generic，保证永远有图。 */
export function spriteFor(agentName: string | null | undefined): SpriteKey {
  const name = (agentName ?? "").toLowerCase();
  if (name.includes("claude")) return "claude";
  if (name.includes("codex")) return "codex";
  if (name.includes("opencode")) return "opencode";
  if (name.includes("hermes")) return "hermes";
  return "generic";
}

export function spriteRows(agentName: string | null | undefined): readonly string[] {
  return SPRITES[spriteFor(agentName)];
}

export interface PixelCell {
  key: string;
  x: number;
  y: number;
}

/** 把网格展开成 SVG rect 列表；每格一个 rect，保证像素边缘不被抗锯齿糊掉。 */
export function spriteCells(agentName: string | null | undefined): PixelCell[] {
  const rows = spriteRows(agentName);
  const cells: PixelCell[] = [];
  rows.forEach((row, y) => {
    for (let x = 0; x < row.length; x += 1) {
      if (row[x] === "#") cells.push({ key: `${x}-${y}`, x, y });
    }
  });
  return cells;
}
