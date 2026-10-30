/**
 * 外观的单一来源：主岛只有明暗两态，同一套设计系统、同一套尺寸与色相分配。
 *
 * 旧版有三套"主题风格"（石墨玻璃 / 纯黑 / 终端），它们各自改圆角、字体与玻璃效果，
 * 结果同一块面板在三种风格下是三个不同的产品。现在收敛为：
 * - 色相只表示状态（工作中 / 已完成 / 等待 / 报错），不再有品牌色；
 * - 明暗只改变底色与墨色，结构、字号、圆角、动效完全一致。
 *
 * 存储键换成 v1 的新键；读到旧键的三种旧值统一落到深色，不丢用户选择（都是深色取向）。
 */

export const APPEARANCES = ["dark", "light"] as const;

export type Appearance = (typeof APPEARANCES)[number];

export const APPEARANCE_LABELS: Record<Appearance, string> = {
  dark: "深色",
  light: "浅色",
};

export const APPEARANCE_STORAGE_KEY = "agent-island-appearance-v1";

/** 旧键：只读一次用于迁移，不再写入。 */
export const LEGACY_THEME_STORAGE_KEY = "agent-island-theme-style-v2";

export function isAppearance(value: unknown): value is Appearance {
  return typeof value === "string" && (APPEARANCES as readonly string[]).includes(value);
}

interface AppearanceStorage {
  getItem(key: string): string | null;
}

export function storedAppearance(storage: AppearanceStorage = localStorage): Appearance {
  const raw = storage.getItem(APPEARANCE_STORAGE_KEY);
  if (isAppearance(raw)) return raw;
  // 旧的三种风格都按深色呈现；用户若曾选纯黑/终端，这里保持深色而不是跳成浅色。
  return "dark";
}
