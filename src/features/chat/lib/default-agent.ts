// Modified by Quotatlas from upstream Atlas (Apache-2.0).
// Which coding agent a BRAND-NEW chat starts on.
//
// The native agent, unconditionally. It is in-process, so it needs no install,
// no sign-in and no probe — it is the one agent a fresh profile is guaranteed
// to have (ADR-0002: Atlas ships no ACP agents).
//
// This used to start on Claude Code whenever a probe said it was installed and
// authenticated, falling back otherwise. Two things were wrong with that. It
// named an agent a fresh install does not have — and the agent switcher lives
// inside the composer that agent's absence disables, so the user could not
// switch away from it either. And the probe was asynchronous, which made a
// first-ever launch hold off creating the session at all until it settled.
//
// Claude Code becomes eligible the moment the user installs it, by switching to
// it like any other agent. Nothing here decides that for them.

import { NATIVE_AGENT_ID, type SwitchableAgent } from "@/types/agent";
import { HOSTED_SERVICES_ENABLED } from "@/lib/quotatlas";
import { useAgentRegistryStore } from "@/features/agents/stores/agent-registry-store";

/** Quotatlas: agents a new chat prefers over the native one, in order. An
 *  account entry (`claude-acp@work`) counts for its base agent. */
const PREFERRED_AGENTS = ["claude-acp", "claude-code", "codex-acp", "codex"];

/** The agent a new chat starts on. Synchronous and total: there is nothing to
 *  probe, so there is no "not decided yet".
 *
 *  Quotatlas: the native agent runs on Atlas's hosted gateway, which Quotatlas
 *  does not use, so a new chat starts on an INSTALLED Claude Code or Codex
 *  agent when there is one. Installed is the whole test — the concern above
 *  was naming an agent the user does not have, and this never does. With
 *  nothing installed it falls back to the native agent, whose composer then
 *  explains what to install. */
export function defaultAgentForNewSession(): SwitchableAgent {
  if (HOSTED_SERVICES_ENABLED) return NATIVE_AGENT_ID;
  const installed = useAgentRegistryStore.getState().catalog.filter((entry) => entry.installed);
  for (const id of PREFERRED_AGENTS) {
    if (installed.some((entry) => entry.id === id)) return id;
  }
  for (const id of PREFERRED_AGENTS) {
    const account = installed.find((entry) => entry.id.startsWith(`${id}@`));
    if (account) return account.id;
  }
  return installed[0]?.id ?? NATIVE_AGENT_ID;
}
