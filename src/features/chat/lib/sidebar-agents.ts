/**
 * Which agent a history row belongs to, and which agent resumes it.
 *
 * Thread rows carry registry ids and older native ids; the sidebar folds them
 * into one **band** per agent (icon, grouping), and resuming a row spawns the
 * registry entry that owns its band. Kept out of the sidebar component so the
 * UI actions' "open thread" resumes through exactly the same table.
 */

import type { SwitchableAgent } from "@/types/agent";

/** Short per-row agent tag. "claude" doubles as the legacy default for rows
 *  with no metadata, so the mapping from AgentType is centralised here instead
 *  of repeated ternaries that silently mislabel new agents. */
export type SidebarAgent =
  | "claude"
  | "codex"
  | "opencode"
  | "cursor"
  | "kilo"
  | "atlas-agent"
  | (string & {});

export function sidebarAgentOf(agentType: string | undefined): SidebarAgent {
  // The registry ids and the older native ids a thread row may carry fold
  // into one band per agent, or the row icon and resume routing split.
  if (agentType === "codex-acp") return "codex";
  if (
    agentType === "codex" ||
    agentType === "opencode" ||
    agentType === "cursor" ||
    agentType === "kilo" ||
    agentType === "atlas-agent"
  )
    return agentType;
  // The real Claude ids only. A `startsWith("claude")` also caught registry
  // agents such as "claude-foo" and resumed their history through claude-acp.
  if (
    !agentType ||
    agentType === "custom" ||
    agentType === "claude" ||
    agentType === "claude-acp" ||
    agentType === "claude-code" ||
    agentType === "claude-code-ts"
  )
    return "claude";
  // Registry-installed external agent: its plugin id IS its identity.
  return agentType;
}

/** Band → the registry id resume must spawn through. The claude/codex bands
 *  fold several ids (see `sidebarAgentOf`), so they need an explicit mapping
 *  back to the registry entries that own those threads. The
 *  old values ("claude-code"/"codex") named plugin ids the registry-only port
 *  deleted, so resuming those rows spawned UnknownSpec — a silent dead click. */
export const AGENT_TYPE_BY_SIDEBAR: Partial<Record<string, SwitchableAgent>> = {
  claude: "claude-acp",
  codex: "codex-acp",
  opencode: "opencode",
  cursor: "cursor",
  kilo: "kilo",
  "atlas-agent": "atlas-agent",
};

/** The agent that resumes a thread recorded under `agentId`. */
export function resumeAgentFor(agentId: string): SwitchableAgent {
  const band = sidebarAgentOf(agentId);
  return AGENT_TYPE_BY_SIDEBAR[band] ?? band;
}
