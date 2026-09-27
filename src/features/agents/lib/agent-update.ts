// Updating an installed agent — the one action behind both Update buttons
// (Settings → Agents, and the bar above a chat's composer), and the phase
// text both show while it runs.
//
// The backend does the work either way: a registry bump updates agents on its
// own (idle ones in the background, running ones once their reply finishes),
// and `acp_registry_update` does the same steps on demand. Its progress
// arrives as `agent_update` events on `atlas:agents`, which App.tsx records
// with `setAgentUpdatePhase`; this file only starts an update and names what
// a phase means to a person.

import { toast } from "sonner";
import { acpRegistry } from "./agent-registry-api";
import {
  hydrateAgentRegistry,
  setAgentUpdatePhase,
  useAgentRegistryStore,
  type AgentUpdatePhase,
} from "../stores/agent-registry-store";

/** What an update in flight is doing, in words. The wait for a reply is the
 *  part that can take minutes, so it is named rather than left as a spinner. */
export function updatePhaseLabel(phase: AgentUpdatePhase["phase"], version: string): string {
  return phase === "waiting" ? "Waiting for the reply to finish…" : `Installing v${version}…`;
}

/** Update `pluginId` to the registry's `version` now. Safe to call twice — a
 *  second call while one is in flight does nothing. Toasts the outcome. */
export async function updateAgent(pluginId: string, name: string, version: string): Promise<void> {
  if (useAgentRegistryStore.getState().updatePhases[pluginId]) return;
  // Shown until the first backend phase replaces it.
  setAgentUpdatePhase(pluginId, { phase: "installing", version });
  try {
    await acpRegistry.update(pluginId);
    // Same id as App.tsx's background-update toast: one update, one toast.
    toast.success(`${name} updated to v${version}`, {
      id: `agent-update:${pluginId}:${version}`,
    });
  } catch (e) {
    toast.error(`Couldn't update ${name}: ${String(e)}`);
  } finally {
    setAgentUpdatePhase(pluginId, null);
  }
  await hydrateAgentRegistry();
}
