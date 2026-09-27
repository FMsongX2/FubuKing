import { invoke } from "@tauri-apps/api/core";
import { acpRegistry } from "@/features/agents/lib/agent-registry-api";

/**
 * Accounts bridge (ADR Q-0002).
 *
 * An account is an extra login of a supported agent, installed as an agent
 * entry of its own. The CLI keeps its settings and credentials in the
 * account's profile home; nothing here ever carries a credential.
 */

/** Which CLI an account drives. */
export type AccountProvider = "claude" | "codex";

export interface Account {
  /** The agent id sessions run under, e.g. `claude-acp@work`. */
  id: string;
  baseAgentId: string;
  label: string;
  provider: AccountProvider;
  /** Where the CLI keeps this account's settings and login. */
  profileHome: string;
  /** The name pickers show, once the registry has resolved the base agent. */
  displayName: string | null;
}

/** The base agents accounts can be created for, as registry ids. */
export const ACCOUNT_BASES: ReadonlyArray<{ id: string; provider: AccountProvider; name: string }> =
  [
    { id: "claude-acp", provider: "claude", name: "Claude Code" },
    { id: "codex-acp", provider: "codex", name: "Codex" },
  ];

export const accounts = {
  list: () => invoke<Account[]>("accounts_list"),
  create: (baseAgentId: string, label: string) =>
    invoke<Account>("accounts_create", { baseAgentId, label }),
  /** Removes the agent entry and drops its connection. The profile home, and
   *  the login inside it, stay on disk so a re-created account signs straight
   *  back in. */
  remove: (id: string) => acpRegistry.uninstall(id, false),
};
