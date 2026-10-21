/**
 * 额度预警：用量百分比越过阈值时提醒一次，同一个"窗口"里不重复轰炸。
 *
 * 数据来自快照里的 `usage.used_percent`（Claude/Codex 的额度窗口），不额外读文件。
 * 「窗口」优先用 `resets_at_secs`（额度重置时间），没有就按当天算。
 */

export const QUOTA_WARN_KEY = "agent-island-quota-warned";
export const DEFAULT_QUOTA_THRESHOLD = 80;

export interface QuotaUsage {
  used_percent: number | null;
  resets_at_secs?: number | null;
  stale?: boolean;
}

/** 额度窗口的稳定标识：重置时间优先，否则按日期。 */
export function quotaWindowKey(usage: QuotaUsage | null | undefined, nowMs: number): string | null {
  if (!usage || usage.used_percent === null || usage.used_percent === undefined) return null;
  if (typeof usage.resets_at_secs === "number" && Number.isFinite(usage.resets_at_secs)) {
    return `reset-${Math.round(usage.resets_at_secs)}`;
  }
  const date = new Date(nowMs);
  return `day-${date.getFullYear()}-${date.getMonth() + 1}-${date.getDate()}`;
}

export interface QuotaWarnInput {
  agentId: string;
  usage: QuotaUsage | null | undefined;
  nowMs: number;
  threshold?: number;
  /** 上次提醒的标记，形如 `<agentId>@<windowKey>` */
  lastWarned: string | null;
}

/** 该不该提醒：用量够高、数据不是过期的、同一个窗口没提醒过。 */
export function shouldWarnQuota({ agentId, usage, nowMs, threshold = DEFAULT_QUOTA_THRESHOLD, lastWarned }: QuotaWarnInput): boolean {
  if (!usage || usage.used_percent === null || usage.used_percent === undefined) return false;
  if (usage.stale) return false;
  if (usage.used_percent < threshold) return false;
  const window = quotaWindowKey(usage, nowMs);
  if (!window) return false;
  return lastWarned !== `${agentId}@${window}`;
}

export function quotaWarnMark(agentId: string, usage: QuotaUsage, nowMs: number): string | null {
  const window = quotaWindowKey(usage, nowMs);
  return window ? `${agentId}@${window}` : null;
}

export function quotaMessage(percent: number, resetsAtSecs: number | null | undefined, nowMs: number): string {
  const rounded = Math.round(percent);
  if (typeof resetsAtSecs === "number" && Number.isFinite(resetsAtSecs)) {
    const minutes = Math.max(0, Math.round((resetsAtSecs * 1000 - nowMs) / 60_000));
    const when = minutes >= 60 ? `${Math.round(minutes / 60)} 小时后` : `${minutes} 分钟后`;
    return `额度已用 ${rounded}%，${when}重置`;
  }
  return `额度已用 ${rounded}%`;
}
