/** 主题风格的单一来源：新增主题只改这里 + 一张 themes/*.css。 */
export const THEME_STYLES = ["graphite", "pure-black", "terminal"] as const;

export type ThemeStyle = (typeof THEME_STYLES)[number];

export const THEME_LABELS: Record<ThemeStyle, string> = {
  graphite: "石墨玻璃",
  "pure-black": "纯黑",
  terminal: "终端",
};

export const THEME_STORAGE_KEY = "agent-island-theme-style-v2";

export function isThemeStyle(value: unknown): value is ThemeStyle {
  return typeof value === "string" && (THEME_STYLES as readonly string[]).includes(value);
}

export function storedThemeStyle(): ThemeStyle {
  const raw = localStorage.getItem(THEME_STORAGE_KEY);
  return isThemeStyle(raw) ? raw : "graphite";
}
