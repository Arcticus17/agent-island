/**
 * 花费追踪：把快照里的 `usage.cost_usd`（当前窗口累计花费）换算成"今天花了多少"。
 *
 * 做法是最朴素的增量累加：记住每个 Agent 上次看到的 cost，新值更大就加上差值；
 * 变小（换会话/窗口重置）就把基线拉回新值，不产生负数。
 *
 * 局限（界面上要如实写）：只统计**岛在线期间**观察到的增量，关掉应用的那段时间不计。
 */

export const SPEND_STORAGE_KEY = "agent-island-spend-daily";
export const BUDGET_STORAGE_KEY = "agent-island-daily-budget";

export interface SpendState {
  /** YYYY-MM-DD（本地时区） */
  date: string;
  /** 每个 Agent 今天累计的花费（美元） */
  totals: Record<string, number>;
  /** 每个 Agent 上次观察到的窗口累计值 */
  baselines: Record<string, number>;
}

export function localDateKey(nowMs: number): string {
  const date = new Date(nowMs);
  const month = String(date.getMonth() + 1).padStart(2, "0");
  const day = String(date.getDate()).padStart(2, "0");
  return `${date.getFullYear()}-${month}-${day}`;
}

export function emptySpendState(nowMs: number): SpendState {
  return { date: localDateKey(nowMs), totals: {}, baselines: {} };
}

/**
 * 记录一次观察：`costUsd` 为该 Agent 当前窗口的累计花费（null 表示没有数据，直接忽略）。
 * 返回新的状态；`delta` 是本次新增的花费。
 */
export function recordCost(
  state: SpendState | null,
  agentId: string,
  costUsd: number | null | undefined,
  nowMs: number,
): { state: SpendState; delta: number } {
  const today = localDateKey(nowMs);
  const base = state && state.date === today ? state : emptySpendState(nowMs);
  if (typeof costUsd !== "number" || !Number.isFinite(costUsd) || costUsd < 0) {
    return { state: base, delta: 0 };
  }
  const previous = base.baselines[agentId];
  const delta = typeof previous === "number" && costUsd > previous ? costUsd - previous : 0;
  return {
    state: {
      date: base.date,
      totals: {
        ...base.totals,
        [agentId]: Number(((base.totals[agentId] ?? 0) + delta).toFixed(6)),
      },
      baselines: { ...base.baselines, [agentId]: costUsd },
    },
    delta,
  };
}

export function dailyTotal(state: SpendState | null, nowMs: number): number {
  if (!state || state.date !== localDateKey(nowMs)) return 0;
  return Object.values(state.totals).reduce((sum, value) => sum + value, 0);
}

export type BudgetState = "off" | "ok" | "near" | "over";

export interface BudgetStatus {
  state: BudgetState;
  spent: number;
  budget: number;
  /** 0-1，预算为 0 时是 0 */
  ratio: number;
}

/** 预算使用情况：>=80% 算 near，>100% 算 over。 */
export function budgetStatus(spent: number, budget: number): BudgetStatus {
  if (!Number.isFinite(budget) || budget <= 0) {
    return { state: "off", spent, budget: 0, ratio: 0 };
  }
  const ratio = spent / budget;
  const state: BudgetState = ratio > 1 ? "over" : ratio >= 0.8 ? "near" : "ok";
  return { state, spent, budget, ratio };
}

export function formatUsd(value: number): string {
  if (!Number.isFinite(value)) return "$0.00";
  const fixed = value.toFixed(value >= 10 ? 1 : 2);
  // $10.0 → $10，别让整数预算显示成小数
  return `$${fixed.endsWith(".0") ? fixed.slice(0, -2) : fixed}`;
}

/** 预算档位（设置里用），0 表示关闭。 */
export const BUDGET_OPTIONS = [0, 5, 10, 20, 50] as const;

export function storedBudget(read: (key: string) => string | null): number {
  const raw = Number(read(BUDGET_STORAGE_KEY));
  return Number.isFinite(raw) && raw > 0 ? raw : 0;
}

