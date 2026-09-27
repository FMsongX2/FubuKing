// @vitest-environment happy-dom
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

// The settings store subscribes to config events when it loads.
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}), emit: vi.fn() }));

import { registerActionHandlers } from "@/features/keybindings/lib/action-registry";
import { performUiAction } from "./ui-actions";
import { seedWindow, uiRequest } from "./test-fixtures";

let unregister: () => void = () => {};
const toggleTerminal = vi.fn();
const nextTab = vi.fn();

beforeEach(() => {
  seedWindow();
  toggleTerminal.mockClear();
  unregister = registerActionHandlers(() => ({
    "panels.terminal": toggleTerminal,
    "tabs.next": nextTab,
  }));
});
afterEach(() => unregister());

describe("ui_command", () => {
  it("runs a global command through the same closure its chord runs", async () => {
    expect(await performUiAction(uiRequest("ui_command", { id: "panels.terminal" }))).toEqual({
      ok: true,
      result: { id: "panels.terminal", ran: true },
    });
    expect(toggleTerminal).toHaveBeenCalledOnce();
  });

  it("refuses an unknown id and lists what can run", async () => {
    const reply = await performUiAction(uiRequest("ui_command", { id: "panels.nope" }));
    expect(reply.ok).toBe(false);
    expect(!reply.ok && reply.error).toMatch(/panels\.terminal, tabs\.next/);
  });

  /// Focus-scoped commands only mean something on their surface.
  it("refuses a focus-scoped command, saying why", async () => {
    const reply = await performUiAction(uiRequest("ui_command", { id: "terminal.find" }));
    expect(!reply.ok && reply.error).toMatch(/focus/);
  });

  /// These would slip past a rule the other tools enforce: switching
  /// projects, closing unsaved work, switching the agent's own chat, and an
  /// agent changing its own permission mode.
  it.each([
    ["workspace.add", /never switch projects/],
    ["tabs.close", /ui_close/],
    ["chat.cycleAgent", /ui_chat/],
    ["chat.cyclePermissionMode", /permission/],
  ])("refuses %s even though it is a global command", async (id, why) => {
    const run = vi.fn();
    unregister();
    unregister = registerActionHandlers(() => ({ [id]: run }));
    const reply = await performUiAction(uiRequest("ui_command", { id }));
    expect(!reply.ok && reply.error).toMatch(why);
    expect(run).not.toHaveBeenCalled();
  });

  it("refuses a global command nobody registered, saying so", async () => {
    const reply = await performUiAction(uiRequest("ui_command", { id: "split.new" }));
    expect(!reply.ok && reply.error).toMatch(/not available right now/);
  });

  it("requires an id", async () => {
    const reply = await performUiAction(uiRequest("ui_command", {}));
    expect(!reply.ok && reply.error).toMatch(/id/);
  });
});
