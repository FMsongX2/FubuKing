/**
 * Accounts (ADR Q-0002) for the browser mock: an in-memory installed map of
 * account entries, so Settings → Accounts can be reviewed without Rust.
 */
import type { Account } from "@/features/accounts/lib/accounts-api";
import type { TypedHandlers } from "../types";

export interface AccountsResponses {
  accounts_list: Account[];
  accounts_create: Account;
}

const BASE_NAMES: Record<string, string> = {
  "claude-acp": "Claude Agent",
  "codex-acp": "Codex",
};

const mockAccounts: Account[] = [
  {
    id: "claude-acp@work",
    baseAgentId: "claude-acp",
    label: "work",
    provider: "claude",
    profileHome: "/Users/dev/.config/fubuking-mock/accounts/claude-acp@work",
    displayName: "Claude Agent · work",
  },
];

function slug(label: string): string {
  const s = label
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .replace(/^-+|-+$/g, "")
    .slice(0, 32);
  return s || "account";
}

export const accountsHandlers: TypedHandlers<AccountsResponses> = {
  accounts_list: (): Account[] => [...mockAccounts],
  accounts_create: ({ baseAgentId, label }): Account => {
    const base = String(baseAgentId);
    const name = String(label).trim();
    if (!name) throw new Error("an account needs a name");
    const stem = `${base}@${slug(name)}`;
    let id = stem;
    for (let n = 2; mockAccounts.some((a) => a.id === id); n++) id = `${stem}-${n}`;
    const account: Account = {
      id,
      baseAgentId: base,
      label: name,
      provider: base.startsWith("claude") ? "claude" : "codex",
      profileHome: `/Users/dev/.config/fubuking-mock/accounts/${id}`,
      displayName: `${BASE_NAMES[base] ?? base} · ${name}`,
    };
    mockAccounts.push(account);
    return account;
  },
};
