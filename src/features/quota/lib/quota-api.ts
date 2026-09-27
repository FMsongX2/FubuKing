import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { AccountProvider } from "@/features/accounts/lib/accounts-api";

/**
 * Quota bridge (ADR Q-0002).
 *
 * Rust reads every account's limits from channels the official CLIs expose
 * and broadcasts each snapshot; this side only renders it.
 */

export type QuotaStatus = "ok" | "pending" | "signed-out" | "untracked" | "unavailable";

export interface QuotaWindow {
  /** Stable within its account, e.g. `codex-weekly`. */
  id: string;
  label: string;
  /** 0 to 100. */
  usedPercent: number;
  windowMinutes: number | null;
  /** Unix seconds. */
  resetsAt: number | null;
}

export interface AccountQuota {
  /** The agent id sessions run under, or `default:<provider>`. */
  accountId: string;
  provider: AccountProvider;
  label: string;
  displayName: string;
  isDefault: boolean;
  plan: string | null;
  status: QuotaStatus;
  message: string | null;
  windows: QuotaWindow[];
  /** Unix seconds the figures describe. */
  updatedAt: number | null;
}

export const QUOTA_CHANGED_EVENT = "atlas:quota-changed";

export const quota = {
  /** The last snapshot, without reading anything. */
  snapshot: () => invoke<AccountQuota[]>("quota_snapshot"),
  /** Read every account now. */
  refresh: () => invoke<AccountQuota[]>("quota_refresh"),
  onChanged: (handler: (quotas: AccountQuota[]) => void): Promise<UnlistenFn> =>
    listen<AccountQuota[]>(QUOTA_CHANGED_EVENT, (e) => handler(e.payload)),
};
