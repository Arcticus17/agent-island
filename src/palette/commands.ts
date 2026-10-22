/**
 * 命令面板（Ctrl+K）：把"点来点去"的操作变成一个键盘可达的搜索框。
 *
 * 数据都是现成的（Agents、会话、几个固定动作），所以这里只负责两件事：
 * 1. 把状态拼成可执行命令列表；
 * 2. 对查询做子序列匹配 + 打分排序（打分规则简单但稳定，便于测试）。
 */

export type PaletteKind = "agent" | "session" | "action";

export interface PaletteCommand {
  id: string;
  kind: PaletteKind;
  title: string;
  hint: string;
}

export interface PaletteSource {
  agents: ReadonlyArray<{
    id: string;
    name: string;
    display_status: string;
    active_session: { id: string; name: string } | null;
    history_sessions: ReadonlyArray<{ id: string; name: string }>;
  }>;
  selectedAgentId: string | null;
}

/** 可执行的固定动作（App 负责把它们映射到真实处理函数）。 */
export const PALETTE_ACTIONS: ReadonlyArray<{ id: string; title: string; hint: string }> = [
  { id: "action:terminal", title: "打开内置终端", hint: "终端" },
  { id: "action:refresh", title: "刷新快照", hint: "刷新" },
  { id: "action:privacy", title: "切换隐私遮罩", hint: "隐私" },
  { id: "action:focus", title: "切换专注模式", hint: "专注" },
  { id: "action:pin", title: "固定 / 取消固定当前 Agent", hint: "固定" },
  { id: "action:onboarding", title: "查看上手引导", hint: "帮助" },
  { id: "action:hide", title: "收起灵动岛", hint: "Esc" },
];

export function buildPaletteCommands(source: PaletteSource): PaletteCommand[] {
  const commands: PaletteCommand[] = [];
  for (const agent of source.agents) {
    commands.push({
      id: `agent:${agent.id}`,
      kind: "agent",
      title: agent.name,
      hint: agent.display_status === "working" ? "工作中" : agent.display_status === "waiting" ? "等待确认" : agent.display_status,
    });
    const selected = agent.id === source.selectedAgentId;
    if (!selected) continue;
    if (agent.active_session) {
      commands.push({
        id: `session:${agent.active_session.id}`,
        kind: "session",
        title: agent.active_session.name || agent.active_session.id,
        hint: "当前会话",
      });
    }
    for (const session of agent.history_sessions) {
      if (session.id === agent.active_session?.id) continue;
      commands.push({
        id: `session:${session.id}`,
        kind: "session",
        title: session.name || session.id,
        hint: "历史会话",
      });
    }
  }
  for (const action of PALETTE_ACTIONS) {
    commands.push({ id: action.id, kind: "action", title: action.title, hint: action.hint });
  }
  return commands;
}

/**
 * 子序列匹配：查询里的字符按顺序出现即命中（大小写不敏感）。
 * 连续命中、词首命中、越短的标题分越高——足以把"终端"排到"刷新快照"前面。
 */
export function scoreCommand(query: string, title: string): number | null {
  const needle = query.trim().toLowerCase();
  if (!needle) return 0;
  const haystack = title.toLowerCase();
  let cursor = 0;
  let score = 0;
  let streak = 0;
  for (const char of needle) {
    const found = haystack.indexOf(char, cursor);
    if (found < 0) return null;
    const isWordStart = found === 0 || /[\s·/:_-]/.test(haystack[found - 1]);
    score += 2;
    if (found === cursor) {
      streak += 1;
      score += 3 + streak;
    } else {
      streak = 0;
    }
    if (isWordStart) score += 2;
    cursor = found + 1;
  }
  return score - Math.min(haystack.length, 60) * 0.05;
}

export function filterCommands(commands: readonly PaletteCommand[], query: string): PaletteCommand[] {
  const scored = commands
    .map((command, index) => ({ command, index, score: scoreCommand(query, command.title) }))
    .filter((entry): entry is { command: PaletteCommand; index: number; score: number } => entry.score !== null)
    .sort((left, right) => (right.score - left.score) || (left.index - right.index));
  return scored.map((entry) => entry.command);
}

export function moveSelection(index: number, delta: number, length: number): number {
  if (length === 0) return 0;
  const next = index + delta;
  if (next < 0) return length - 1;
  if (next >= length) return 0;
  return next;
}


