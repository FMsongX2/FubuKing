import type { AccountQuota, QuotaWindow } from "./quota-api";

/**
 * Pure helpers behind the quota views: how much is left, how urgent that is,
 * and how long until a window resets.
 */

/** Green, orange, red: the thresholds Quotio uses (above 50%, above 20%). */
export type QuotaLevel = "good" | "low" | "critical";

export function remainingPercent(window: Pick<QuotaWindow, "usedPercent">): number {
  return Math.round(Math.min(100, Math.max(0, 100 - window.usedPercent)));
}

export function quotaLevel(remaining: number): QuotaLevel {
  if (remaining > 50) return "good";
  if (remaining > 20) return "low";
  return "critical";
}

/** "2d 17h", "4h 59m", "12m", or "now", counting down to `resetsAt`. */
export function formatCountdown(
  resetsAt: number | null,
  nowMs: number = Date.now(),
): string | null {
  if (resetsAt === null) return null;
  const minutes = Math.floor((resetsAt * 1000 - nowMs) / 60_000);
  if (minutes <= 0) return "now";
  const days = Math.floor(minutes / 1_440);
  const hours = Math.floor((minutes % 1_440) / 60);
  const mins = minutes % 60;
  if (days > 0) return hours > 0 ? `${days}d ${hours}h` : `${days}d`;
  if (hours > 0) return mins > 0 ? `${hours}h ${mins}m` : `${hours}h`;
  return `${mins}m`;
}

/** "pro" → "Pro", "API key" unchanged. */
export function formatPlan(plan: string | null): string | null {
  if (!plan) return null;
  return plan.charAt(0).toUpperCase() + plan.slice(1);
}

/** The window with the least left, which is the one that will stop you. */
export function tightestWindow(account: AccountQuota): QuotaWindow | null {
  let tightest: QuotaWindow | null = null;
  for (const window of account.windows) {
    if (!tightest || remainingPercent(window) < remainingPercent(tightest)) tightest = window;
  }
  return tightest;
}

/** The worst level among accounts with figures, for a provider's status dot. */
export function worstLevel(accounts: AccountQuota[]): QuotaLevel | null {
  const order: QuotaLevel[] = ["good", "low", "critical"];
  let worst: QuotaLevel | null = null;
  for (const account of accounts) {
    const window = account.status === "ok" ? tightestWindow(account) : null;
    if (!window) continue;
    const level = quotaLevel(remainingPercent(window));
    if (!worst || order.indexOf(level) > order.indexOf(worst)) worst = level;
  }
  return worst;
}

export const LEVEL_TEXT: Record<QuotaLevel, string> = {
  good: "text-success",
  low: "text-warning",
  critical: "text-destructive",
};

export const LEVEL_FILL: Record<QuotaLevel, string> = {
  good: "bg-success",
  low: "bg-warning",
  critical: "bg-destructive",
};
