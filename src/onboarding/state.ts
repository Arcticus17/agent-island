/**
 * 首次引导：只在第一次打开时出现一次。
 *
 * 岛默认是隐藏的一条小缝，第一次用的人几乎不可能自己发现
 * "Ctrl+Shift+A 唤出 / Ctrl+K 打开命令面板"。所以给一次性的上手卡片，
 * 看过就记一笔，之后不再打扰（命令面板里可以随时重看）。
 */

export const ONBOARDED_KEY = "agent-island-onboarded";

export interface OnboardingStep {
  keys: string;
  title: string;
  detail: string;
}

export const ONBOARDING_STEPS: readonly OnboardingStep[] = [
  { keys: "Ctrl+Shift+A", title: "唤出 / 收起", detail: "平时它藏在屏幕顶部，用这个组合键叫出来" },
  { keys: "Ctrl+K", title: "命令面板", detail: "切 Agent、切会话、开终端，键盘走完" },
  { keys: "允许 / 记住这条", title: "处理审批", detail: "允许一次，或者把这类命令加进白名单" },
];

export function shouldShowOnboarding(read: (key: string) => string | null): boolean {
  return read(ONBOARDED_KEY) !== "1";
}

export function markOnboarded(write: (key: string, value: string) => void): void {
  write(ONBOARDED_KEY, "1");
}
