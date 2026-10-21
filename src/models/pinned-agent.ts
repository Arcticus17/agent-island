/**
 * Agent 固定：用户盯住某个 Agent 时，自动选择不应该再把它换走。
 *
 * 自动选择本来按"谁最需要看"排序（工作中 > 等待确认 > …），这对巡视很合适，
 * 但一旦你决定盯住某个会话，每隔几秒被换走就很难受。
 */

export const PINNED_AGENT_KEY = "agent-island-pinned-agent";

export interface PinnableAgent {
  id: string;
  name: string;
}

/** 固定的 Agent 还在可见列表里才算数；消失了就当作没固定。 */
export function resolvePinnedAgent<T extends PinnableAgent>(
  agents: readonly T[],
  pinnedId: string | null | undefined,
): T | null {
  if (!pinnedId) return null;
  return agents.find((agent) => agent.id === pinnedId) ?? null;
}

/**
 * 下一次应该选中谁：
 * - 固定命中 → 固定项（哪怕别的 Agent 更"忙"）
 * - 固定项消失 → null，并提示调用方解除固定
 * - 没固定 → 交给自动选择（调用方决定）
 */
export function pinnedSelection<T extends PinnableAgent>(
  agents: readonly T[],
  pinnedId: string | null | undefined,
): { agent: T | null; stalePin: boolean } {
  if (!pinnedId) return { agent: null, stalePin: false };
  const agent = resolvePinnedAgent(agents, pinnedId);
  return { agent, stalePin: agent === null };
}

export function storedPinnedAgent(read: (key: string) => string | null): string | null {
  const raw = read(PINNED_AGENT_KEY);
  return raw && raw.trim() ? raw : null;
}
