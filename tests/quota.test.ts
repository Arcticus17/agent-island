import { describe, expect, it } from "vitest";

import { quotaMessage, quotaWarnMark, quotaWindowKey, shouldWarnQuota } from "../src/usage/quota";

const NOW = Date.UTC(2026, 9, 21, 10, 0, 0);
const DAY = 24 * 60 * 60;

describe("quota warnings", () => {
  it("keys the window by reset time, falling back to the day", () => {
    expect(quotaWindowKey({ used_percent: 50, resets_at_secs: NOW / 1000 + 3600 }, NOW)).toBe(
      `reset-${Math.round(NOW / 1000 + 3600)}`,
    );
    expect(quotaWindowKey({ used_percent: 50, resets_at_secs: null }, NOW)).toBe("day-2026-10-21");
    expect(quotaWindowKey({ used_percent: null }, NOW)).toBeNull();
    expect(quotaWindowKey(null, NOW)).toBeNull();
  });

  it("warns once per window above the threshold", () => {
    const usage = { used_percent: 85, resets_at_secs: NOW / 1000 + 3600 };
    const mark = quotaWarnMark("claude", usage, NOW)!;
    expect(shouldWarnQuota({ agentId: "claude", usage, nowMs: NOW, lastWarned: null })).toBe(true);
    expect(shouldWarnQuota({ agentId: "claude", usage, nowMs: NOW, lastWarned: mark })).toBe(false);
    // 换了窗口（重置时间变了）要重新提醒
    const nextWindow = { used_percent: 85, resets_at_secs: NOW / 1000 + 2 * 3600 };
    expect(shouldWarnQuota({ agentId: "claude", usage: nextWindow, nowMs: NOW, lastWarned: mark })).toBe(true);
  });

  it("stays quiet below the threshold, for stale data, and for missing usage", () => {
    expect(shouldWarnQuota({ agentId: "a", usage: { used_percent: 79 }, nowMs: NOW, lastWarned: null })).toBe(false);
    expect(shouldWarnQuota({ agentId: "a", usage: { used_percent: 95, stale: true }, nowMs: NOW, lastWarned: null })).toBe(false);
    expect(shouldWarnQuota({ agentId: "a", usage: { used_percent: null }, nowMs: NOW, lastWarned: null })).toBe(false);
    expect(shouldWarnQuota({ agentId: "a", usage: null, nowMs: NOW, lastWarned: null })).toBe(false);
  });

  it("respects a custom threshold", () => {
    expect(
      shouldWarnQuota({ agentId: "a", usage: { used_percent: 62 }, nowMs: NOW, threshold: 60, lastWarned: null }),
    ).toBe(true);
  });

  it("describes the reset time in human terms", () => {
    expect(quotaMessage(82.4, NOW / 1000 + 3600, NOW)).toBe("额度已用 82%，1 小时后重置");
    expect(quotaMessage(90, NOW / 1000 + 25 * 60, NOW)).toBe("额度已用 90%，25 分钟后重置");
    expect(quotaMessage(88, null, NOW)).toBe("额度已用 88%");
  });
});
