import { afterEach, describe, expect, it, vi } from "vitest";
import {
  hasActionHandler,
  registerActionHandlers,
  runAction,
  runnableActionIds,
  type ActionHandlers,
} from "./action-registry";

let unregister: (() => void) | null = null;
afterEach(() => {
  unregister?.();
  unregister = null;
});

describe("action registry", () => {
  it("runs a registered global command", () => {
    const toggle = vi.fn();
    unregister = registerActionHandlers(() => ({ "panels.terminal": toggle }));
    expect(runAction("panels.terminal")).toEqual({ ok: true });
    expect(toggle).toHaveBeenCalledOnce();
  });

  /// The app shell re-creates its handler closures on every render; the
  /// registry must run the newest one, not the one it was handed first.
  it("runs the latest closure the getter returns", () => {
    let handlers: ActionHandlers = { "panels.terminal": vi.fn() };
    unregister = registerActionHandlers(() => handlers);
    const newer = vi.fn();
    handlers = { "panels.terminal": newer };
    runAction("panels.terminal");
    expect(newer).toHaveBeenCalledOnce();
  });

  it("refuses an unknown id", () => {
    expect(runAction("panels.nope")).toEqual({ ok: false, reason: "unknown-id" });
  });

  /// Focus-scoped commands (terminal find, KB save…) only mean something on
  /// their surface; running one from nowhere would silently do nothing.
  it("refuses a command that is not global", () => {
    unregister = registerActionHandlers(() => ({}));
    expect(runAction("terminal.find")).toEqual({ ok: false, reason: "not-global" });
  });

  it("refuses a global command nobody registered a handler for", () => {
    unregister = registerActionHandlers(() => ({}));
    expect(runAction("panels.terminal")).toEqual({ ok: false, reason: "not-registered" });
  });

  it("forgets the handlers on unregister", () => {
    const toggle = vi.fn();
    registerActionHandlers(() => ({ "panels.terminal": toggle }))();
    expect(hasActionHandler("panels.terminal")).toBe(false);
    expect(runAction("panels.terminal")).toEqual({ ok: false, reason: "not-registered" });
  });

  it("an old unregister leaves a newer registration in place", () => {
    const dropOld = registerActionHandlers(() => ({}));
    const toggle = vi.fn();
    unregister = registerActionHandlers(() => ({ "panels.terminal": toggle }));
    dropOld();
    expect(runAction("panels.terminal")).toEqual({ ok: true });
  });

  it("lists exactly the global commands that have a handler", () => {
    unregister = registerActionHandlers(() => ({
      "panels.terminal": vi.fn(),
      "tabs.next": vi.fn(),
      "terminal.find": vi.fn(),
    }));
    expect(runnableActionIds().sort()).toEqual(["panels.terminal", "tabs.next"]);
  });
});
