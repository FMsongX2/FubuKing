// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

// The settings store subscribes to config events when it loads.
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}), emit: vi.fn() }));

import { useLogStore } from "@/features/log/stores/log-store";
import { useSettingsStore } from "@/features/settings/stores/settings-store";
import { DEFAULT_SETTINGS } from "@/features/settings/lib/app-settings";
import { performUiAction } from "./ui-actions";
import { seedWindow, uiRequest } from "./test-fixtures";

beforeEach(() => {
  seedWindow();
  useLogStore.setState({ buffer: [] });
  useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS } });
});

describe("performUiAction", () => {
  it("answers ui_state with the window's state", async () => {
    const reply = await performUiAction(uiRequest("ui_state"));
    expect(reply.ok).toBe(true);
    expect(reply.ok && (reply.result as { activeTabId: string }).activeTabId).toBe(
      "editor:/p/src/App.tsx",
    );
  });

  it("is on by default", () => {
    expect(DEFAULT_SETTINGS.agentUiNavigation).toBe(true);
  });

  /// Rust refuses first; this is the window's own guard, for a request that
  /// raced the setting being switched off.
  it("refuses everything while the user has switched navigation off", async () => {
    useSettingsStore.setState({ settings: { ...DEFAULT_SETTINGS, agentUiNavigation: false } });
    const reply = await performUiAction(uiRequest("ui_state"));
    expect(reply.ok).toBe(false);
    expect(!reply.ok && reply.error).toContain("switched off");
  });

  it("refuses a tool it does not know, naming it", async () => {
    expect(await performUiAction(uiRequest("ui_teleport"))).toEqual({
      ok: false,
      error: 'unknown UI action "ui_teleport"',
    });
  });

  /// The Logs panel is the audit trail: one row per UI action, saying which
  /// agent did what.
  it("writes one audit row per action", async () => {
    await performUiAction(uiRequest("ui_state"));
    const rows = useLogStore.getState().buffer.filter((e) => e.kind === "agent-ui-action");
    expect(rows).toHaveLength(1);
    expect(rows[0].summary).toContain("ui_state");
  });
});
