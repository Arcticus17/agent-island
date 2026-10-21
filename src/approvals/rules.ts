/**
 * 从一条待审批命令推导「允许清单」的模式。
 *
 * 目标是"下次同类命令不用再问"，所以默认放宽到最后一段之前：
 *   npm install left-pad   → npm install *
 *   git status             → git status        （只有一段就不加通配）
 *   rm -rf ./build         → rm -rf *          （危险命令也只用前缀，仍需人工确认一次）
 * 用户在规则抽屉里还能继续改。
 */
export function suggestRulePattern(command: string): string {
  const trimmed = command.trim().replace(/\s+/g, " ");
  if (!trimmed) return "";
  const tokens = trimmed.split(" ");
  if (tokens.length <= 1) return trimmed;
  const head = tokens.slice(0, Math.min(2, tokens.length - 1)).join(" ");
  return `${head} *`;
}

/** 展示用的规则文案。 */
export function ruleLabel(tool: string, pattern: string): string {
  return `${tool.trim() || "任意工具"} · ${pattern}`;
}
