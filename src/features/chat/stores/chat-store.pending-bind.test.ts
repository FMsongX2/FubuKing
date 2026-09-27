// @vitest-environment happy-dom
//
// Deltas route by `session_id`, so a tab that is still binding (no session
// id yet) never heard that the agent it was waiting for had died. The
// plugin-keyed path: the held first message returns to the queue, the status
// drops to idle, and the tab is flagged so the Restart banner shows.

import { beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@tauri-apps/api/core", () => ({
  invoke: vi.fn(async () => undefined),
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(async () => {}),
}));

import { pluginIdForAgent } from "@/types/agent";
import { useChatStore } from "./chat-store";

const STARTING = "tab-starting";
const BOUND = "tab-bound";
const OTHER = "tab-other-agent";

describe("failPendingBinds", () => {
  beforeEach(() => {
    localStorage.clear();
    useChatStore.setState({
      sessions: {},
      queues: {},
      agentStartingStatus: {},
      activeSessionId: null,
    });
    const { actions } = useChatStore.getState();
    actions.createSession(STARTING, "codex");
    actions.addMessage(STARTING, "user", "first");
    actions.updateSessionStatus(STARTING, "running");
    actions.setPendingSend(STARTING, {
      content: "first",
      mentions: [],
      attachments: [],
    });
    actions.createSession(BOUND, "codex");
    actions.setAcpBinding(BOUND, "agent-1", "acp-1", "/tmp");
    actions.updateSessionStatus(BOUND, "running");
    actions.createSession(OTHER, "claude-code");
    actions.updateSessionStatus(OTHER, "running");
    actions.setPendingSend(OTHER, {
      content: "other",
      mentions: [],
      attachments: [],
    });
  });

  it("moves the held message to the queue and flags only the unbound tabs on that plugin", () => {
    const { actions } = useChatStore.getState();
    actions.setAgentStartingStatus(pluginIdForAgent("codex"), "Installing codex-acp…");
    actions.failPendingBinds(pluginIdForAgent("codex"), "exited with code 1");

    const s = useChatStore.getState();
    expect(s.sessions[STARTING].pendingSend).toBeUndefined();
    expect(s.queues[STARTING]).toEqual(["first"]);
    expect(s.sessions[STARTING].status).toBe("idle");
    expect(s.sessions[STARTING].disconnected).toBe(true);
    expect(s.sessions[STARTING].bindError).toBe("exited with code 1");
    expect(s.agentStartingStatus[pluginIdForAgent("codex")]).toBeUndefined();

    // Bound tab: routed by session id elsewhere, untouched here.
    expect(s.sessions[BOUND].status).toBe("running");
    expect(s.sessions[BOUND].disconnected).toBeUndefined();
    // Different plugin: untouched.
    expect(s.sessions[OTHER].pendingSend?.content).toBe("other");
    expect(s.sessions[OTHER].status).toBe("running");
  });

  it("a later bind clears the recorded reason", () => {
    const { actions } = useChatStore.getState();
    actions.failPendingBinds(pluginIdForAgent("codex"), "boom");
    actions.setAcpBinding(STARTING, "agent-2", "acp-2", "/tmp");
    expect(useChatStore.getState().sessions[STARTING].bindError).toBeUndefined();
  });

  it("noteAgentRemoved flags bound tabs too, and leaves other agents alone", () => {
    // An uninstall drops the connection of a BOUND tab with no delta, so
    // unlike failPendingBinds this must reach tabs that already have a session.
    const { actions } = useChatStore.getState();
    actions.setAgentStartingStatus(pluginIdForAgent("codex"), "Installing codex-acp…");
    actions.noteAgentRemoved(pluginIdForAgent("codex"), "Codex was removed");

    const s = useChatStore.getState();
    for (const tab of [STARTING, BOUND]) {
      expect(s.sessions[tab].status).toBe("idle");
      expect(s.sessions[tab].disconnected).toBe(true);
      expect(s.sessions[tab].bindError).toBe("Codex was removed");
    }
    expect(s.sessions[BOUND].acpSessionId).toBe("acp-1");
    expect(s.sessions[STARTING].pendingSend).toBeUndefined();
    expect(s.queues[STARTING]).toEqual(["first"]);
    expect(s.sessions[OTHER].status).toBe("running");
    expect(s.sessions[OTHER].disconnected).toBeFalsy();
    expect(pluginIdForAgent("codex") in s.agentStartingStatus).toBe(false);
  });

  it("switching a removed agent's tab to another agent clears the disconnected state", () => {
    const { actions } = useChatStore.getState();
    actions.noteAgentRemoved(pluginIdForAgent("codex"), "Codex was removed");
    actions.switchChatAgent(BOUND, "atlas-agent");
    const s = useChatStore.getState().sessions[BOUND];
    expect(s.agentType).toBe("atlas-agent");
    expect(s.disconnected).toBeFalsy();
    expect(s.bindError).toBeUndefined();
  });

  it("setAgentStartingStatus stores text and clears on null", () => {
    const { actions } = useChatStore.getState();
    actions.setAgentStartingStatus("p", "Downloading Node.js…");
    expect(useChatStore.getState().agentStartingStatus.p).toBe("Downloading Node.js…");
    actions.setAgentStartingStatus("p", null);
    expect("p" in useChatStore.getState().agentStartingStatus).toBe(false);
  });
});
