// @vitest-environment happy-dom
import { beforeEach, describe, expect, it, vi } from "vitest";

// The settings store subscribes to config events when it loads.
vi.mock("@tauri-apps/api/event", () => ({ listen: vi.fn(async () => () => {}), emit: vi.fn() }));

import { ownsUiActionRequest } from "./ownership";
import { seedWindow, uiRequest } from "./test-fixtures";

beforeEach(seedWindow);

describe("ownsUiActionRequest", () => {
  it("answers when this window hosts the calling session's chat", () => {
    expect(ownsUiActionRequest(uiRequest("ui_state"), "other-window")).toBe(true);
  });

  it("otherwise only the main window answers", () => {
    const stranger = uiRequest("ui_state", {}, "sess-elsewhere");
    expect(ownsUiActionRequest(stranger, "main")).toBe(true);
    expect(ownsUiActionRequest(stranger, "browser-1")).toBe(false);
  });
});
