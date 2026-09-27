// @vitest-environment happy-dom
//
// `removed-agents` pulls in the chat store, which touches `window` at import.

import { describe, expect, it, vi } from "vitest";
import type { AgentCatalogEntry } from "@/types/agent-catalog";

// This pure helper currently shares a module with the catalog watcher, which
// imports the chat/project stores. Their module setup subscribes to config
// events, so provide the real Tauri boundary shape rather than letting the
// browser shim reject asynchronously after otherwise-passing assertions.
vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
  emit: vi.fn(async () => {}),
}));

import { uninstalledBetween } from "./removed-agents";

function entry(id: string, installed: boolean, kind = "external"): AgentCatalogEntry {
  return { id, agentType: id, installed, kind } as AgentCatalogEntry;
}

describe("uninstalledBetween", () => {
  it("names an external that was installed and no longer is", () => {
    expect(
      uninstalledBetween(
        [entry("atlas-agent", true, "native"), entry("claude-acp", true), entry("codex-acp", true)],
        [entry("atlas-agent", true, "native"), entry("codex-acp", true)],
      ),
    ).toEqual(["claude-acp"]);
  });

  it("treats an entry flipping to installed=false the same as one that vanished", () => {
    expect(uninstalledBetween([entry("claude-acp", true)], [entry("claude-acp", false)])).toEqual([
      "claude-acp",
    ]);
  });

  it("reports nothing for an install, a no-op hydrate, or a detected-only entry", () => {
    expect(
      uninstalledBetween(
        [entry("codex-acp", true)],
        [entry("codex-acp", true), entry("claude-acp", true)],
      ),
    ).toEqual([]);
    expect(uninstalledBetween([entry("codex-acp", true)], [entry("codex-acp", true)])).toEqual([]);
    expect(uninstalledBetween([entry("cursor", false)], [])).toEqual([]);
  });

  it("never reads the first hydrate as a mass uninstall", () => {
    // Pre-hydration the catalog is empty; the native agent alone is not a
    // removal either.
    expect(uninstalledBetween([], [entry("atlas-agent", true, "native")])).toEqual([]);
    expect(uninstalledBetween([entry("atlas-agent", true, "native")], [])).toEqual([]);
  });
});
