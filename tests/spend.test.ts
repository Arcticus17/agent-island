import { describe, expect, it } from "vitest";

import {
  BUDGET_OPTIONS,
  budgetStatus,
  dailyTotal,
  emptySpendState,
  formatUsd,
  localDateKey,
  recordCost,
  storedBudget,
} from "../src/usage/spend";

const NOW = new Date(2026, 9, 21, 12, 0, 0).getTime();
const NEXT_DAY = new Date(2026, 9, 22, 0, 30, 0).getTime();

describe("daily spend tracking", () => {
  it("only counts increases observed while running", () => {
    // 第一次观察只建立基线：窗口累计值里有多少是"今天"的，从外部无从判断
    const first = recordCost(emptySpendState(NOW), "claude", 1.25, NOW);
    expect(first.delta).toBe(0);
    expect(dailyTotal(first.state, NOW)).toBe(0);

    const second = recordCost(first.state, "claude", 1.75, NOW);
    expect(second.delta).toBeCloseTo(0.5);
    expect(dailyTotal(second.state, NOW)).toBeCloseTo(0.5);

    const third = recordCost(second.state, "claude", 1.75, NOW);
    expect(third.delta).toBe(0);
    expect(dailyTotal(third.state, NOW)).toBeCloseTo(0.5);
  });

  it("treats a smaller cost as a new window instead of a negative delta", () => {
    const { state } = recordCost(emptySpendState(NOW), "claude", 4, NOW);
    const reset = recordCost(state, "claude", 0.5, NOW);
    expect(reset.delta).toBe(0);
    expect(dailyTotal(reset.state, NOW)).toBe(0);
    const grown = recordCost(reset.state, "claude", 1.5, NOW);
    expect(grown.delta).toBeCloseTo(1);
    expect(dailyTotal(grown.state, NOW)).toBeCloseTo(1);
  });

  it("keeps agents apart and ignores missing cost", () => {
    let { state } = recordCost(emptySpendState(NOW), "claude", 2, NOW);
    state = recordCost(state, "codex", null, NOW).state;
    state = recordCost(state, "codex", 0.75, NOW).state; // 基线
    state = recordCost(state, "claude", 2.4, NOW).state; // +0.4
    state = recordCost(state, "codex", 1, NOW).state; // +0.25
    expect(state.totals.claude).toBeCloseTo(0.4);
    expect(state.totals.codex).toBeCloseTo(0.25);
    expect(dailyTotal(state, NOW)).toBeCloseTo(0.65);
  });

  it("rolls over at midnight", () => {
    const { state } = recordCost(emptySpendState(NOW), "claude", 3, NOW);
    const grown = recordCost(state, "claude", 4, NOW);
    expect(localDateKey(NOW)).toBe("2026-10-21");
    expect(dailyTotal(grown.state, NOW)).toBeCloseTo(1);
    // 第二天：昨天的累计不算进来，基线重新建立
    expect(dailyTotal(grown.state, NEXT_DAY)).toBe(0);
    const next = recordCost(grown.state, "claude", 4, NEXT_DAY);
    expect(next.state.date).toBe("2026-10-22");
    expect(next.delta).toBe(0);
    expect(dailyTotal(next.state, NEXT_DAY)).toBe(0);
  });

  it("classifies budget states", () => {
    expect(budgetStatus(1, 0).state).toBe("off");
    expect(budgetStatus(1, 10).state).toBe("ok");
    expect(budgetStatus(8, 10).state).toBe("near");
    expect(budgetStatus(10.5, 10).state).toBe("over");
    expect(budgetStatus(8, 10).ratio).toBeCloseTo(0.8);
  });

  it("formats usd and reads the stored budget", () => {
    expect(formatUsd(0)).toBe("$0.00");
    expect(formatUsd(1.234)).toBe("$1.23");
    expect(formatUsd(12.6)).toBe("$12.6");
    expect(formatUsd(10)).toBe("$10");
    expect(storedBudget(() => "10")).toBe(10);
    expect(storedBudget(() => "0")).toBe(0);
    expect(storedBudget(() => null)).toBe(0);
    expect(storedBudget(() => "nope")).toBe(0);
    expect(BUDGET_OPTIONS).toContain(10);
  });
});

