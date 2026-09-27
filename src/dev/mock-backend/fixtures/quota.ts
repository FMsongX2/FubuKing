/**
 * Quota (ADR Q-0002) for the browser mock: one account in each state the
 * Quota tab and the title bar readout render, with countdowns relative to now.
 */
import type { AccountQuota } from "@/features/quota/lib/quota-api";
import type { TypedHandlers } from "../types";

export interface QuotaResponses {
  quota_snapshot: AccountQuota[];
  quota_refresh: AccountQuota[];
}

const inMinutes = (minutes: number) => Math.floor(Date.now() / 1000) + minutes * 60;
const minutesAgo = (minutes: number) => Math.floor(Date.now() / 1000) - minutes * 60;

function mockQuotas(): AccountQuota[] {
  return [
    {
      accountId: "default:claude",
      provider: "claude",
      label: "default",
      displayName: "Claude Code",
      isDefault: true,
      plan: null,
      status: "untracked",
      message:
        "Your default Claude Code login is not tracked, because that would mean editing ~/.claude. Add it as an account to see its limits.",
      windows: [],
      updatedAt: null,
    },
    {
      accountId: "claude-acp@work",
      provider: "claude",
      label: "work",
      displayName: "Claude Agent · work",
      isDefault: false,
      plan: null,
      status: "ok",
      message: null,
      windows: [
        {
          id: "session",
          label: "Session",
          usedPercent: 9,
          windowMinutes: 300,
          resetsAt: inMinutes(225),
        },
        {
          id: "weekly",
          label: "Weekly",
          usedPercent: 71,
          windowMinutes: 10080,
          resetsAt: inMinutes(2 * 1440 + 17 * 60),
        },
      ],
      updatedAt: minutesAgo(3),
    },
    {
      accountId: "claude-acp@side",
      provider: "claude",
      label: "side",
      displayName: "Claude Agent · side",
      isDefault: false,
      plan: null,
      status: "pending",
      message:
        "Claude Code reports limits through its status line. Use this account once in a terminal to load them.",
      windows: [],
      updatedAt: null,
    },
    {
      accountId: "default:codex",
      provider: "codex",
      label: "default",
      displayName: "Codex",
      isDefault: true,
      plan: "pro",
      status: "ok",
      message: null,
      windows: [
        {
          id: "codex-session",
          label: "Session",
          usedPercent: 4,
          windowMinutes: 300,
          resetsAt: inMinutes(46),
        },
        {
          id: "codex-weekly",
          label: "Weekly",
          usedPercent: 89,
          windowMinutes: 10080,
          resetsAt: inMinutes(1449),
        },
        {
          id: "base_model_inference-weekly",
          label: "gpt-reserve Weekly",
          usedPercent: 0,
          windowMinutes: 10080,
          resetsAt: inMinutes(6 * 1440 + 23 * 60),
        },
      ],
      updatedAt: minutesAgo(1),
    },
    {
      accountId: "codex-acp@personal",
      provider: "codex",
      label: "personal",
      displayName: "Codex · personal",
      isDefault: false,
      plan: null,
      status: "signed-out",
      message: "Not signed in. Start a chat with this account to sign in.",
      windows: [],
      updatedAt: null,
    },
  ];
}

export const quotaHandlers: TypedHandlers<QuotaResponses> = {
  quota_snapshot: (): AccountQuota[] => mockQuotas(),
  quota_refresh: (): AccountQuota[] => mockQuotas(),
};
