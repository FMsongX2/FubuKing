import { afterEach, describe, expect, it } from "vitest";
import { useAgentRegistryStore } from "@/features/agents/stores/agent-registry-store";
import type { AgentCatalogEntry } from "@/types/agent-catalog";
import { NATIVE_AGENT_ID } from "@/types/agent";
import { defaultAgentForNewSession } from "./default-agent";

const initial = useAgentRegistryStore.getState().catalog;

function withCatalog(entries: Array<{ id: string; installed: boolean }>) {
  useAgentRegistryStore.setState({
    catalog: entries as unknown as AgentCatalogEntry[],
  });
}

afterEach(() => useAgentRegistryStore.setState({ catalog: initial }));

describe("the agent a new chat starts on (FubuMem)", () => {
  it("falls back to the native agent when nothing is installed", () => {
    withCatalog([{ id: "claude-acp", installed: false }]);
    expect(defaultAgentForNewSession()).toBe(NATIVE_AGENT_ID);
  });

  it("prefers installed Claude Code over Codex", () => {
    withCatalog([
      { id: "codex-acp", installed: true },
      { id: "claude-acp", installed: true },
    ]);
    expect(defaultAgentForNewSession()).toBe("claude-acp");
  });

  it("uses an account when only accounts of a preferred agent are installed", () => {
    withCatalog([
      { id: "gemini", installed: true },
      { id: "codex-acp@work", installed: true },
    ]);
    expect(defaultAgentForNewSession()).toBe("codex-acp@work");
  });

  it("takes any installed agent before the native one", () => {
    withCatalog([{ id: "gemini", installed: true }]);
    expect(defaultAgentForNewSession()).toBe("gemini");
  });
});
