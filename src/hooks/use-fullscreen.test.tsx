// @vitest-environment happy-dom
import { act, cleanup, renderHook, waitFor } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

// vitest runs with `globals: false`, so RTL's auto-cleanup is not registered.
beforeEach(cleanup);

/** Resolvers for in-flight `isFullscreen()` calls, so a test can land them out
 *  of order the way a macOS fullscreen transition does. */
let pending: Array<(value: boolean) => void> = [];
let resizeHandlers: Array<() => void> = [];
let focusHandlers: Array<() => void> = [];

const isFullscreen = vi.fn(() => new Promise<boolean>((resolve) => pending.push(resolve)));

vi.mock("@tauri-apps/api/window", () => ({
  getCurrentWindow: () => ({
    isFullscreen,
    onResized: (handler: () => void) => {
      resizeHandlers.push(handler);
      return Promise.resolve(() => {
        resizeHandlers = resizeHandlers.filter((h) => h !== handler);
      });
    },
    onFocusChanged: (handler: () => void) => {
      focusHandlers.push(handler);
      return Promise.resolve(() => {
        focusHandlers = focusHandlers.filter((h) => h !== handler);
      });
    },
  }),
}));

beforeEach(() => {
  pending = [];
  resizeHandlers = [];
  focusHandlers = [];
  isFullscreen.mockClear();
});

async function mount() {
  const { useFullscreen } = await import("./use-fullscreen");
  const view = renderHook(() => useFullscreen());
  // The initial read, plus subscribing to both events.
  await waitFor(() => expect(pending).toHaveLength(1));
  pending.shift()!(false);
  await waitFor(() => expect(resizeHandlers).toHaveLength(1));
  return view;
}

describe("useFullscreen", () => {
  it("ignores a stale answer that resolves after a newer one", async () => {
    // The bug this guards: macOS animates the fullscreen exit, firing a burst
    // of resize events. Each one asks the window over IPC, and those round
    // trips can come back out of order — so an early `true` landing after the
    // final `false` latched the wrong value with no further event to correct
    // it. The titlebar then dropped its 72px traffic-light inset while the
    // lights were still there.
    const view = await mount();

    // Two resizes from one transition; neither has answered yet.
    resizeHandlers[0]();
    resizeHandlers[0]();
    await waitFor(() => expect(pending).toHaveLength(2));

    const [first, second] = pending;
    // The NEWER query settles first, with the truth.
    await act(async () => second(false));
    await waitFor(() => expect(view.result.current).toBe(false));

    // The older one answers late, and wrongly. Flushed to completion, so this
    // asserts on the settled value rather than on a render that has not yet
    // happened — without the flush the test passes even with the guard removed.
    await act(async () => first(true));
    await act(async () => {
      await Promise.resolve();
    });

    expect(view.result.current).toBe(false);
  });

  it("reports fullscreen when the newest answer says so", async () => {
    const view = await mount();

    resizeHandlers[0]();
    await waitFor(() => expect(pending).toHaveLength(1));
    pending.shift()!(true);

    await waitFor(() => expect(view.result.current).toBe(true));
  });

  it("re-checks when the window regains focus", async () => {
    // The backstop for a transition that completed while this window was not
    // frontmost, where the resize burst can be coalesced away entirely.
    const view = await mount();
    expect(focusHandlers).toHaveLength(1);

    focusHandlers[0]();
    await waitFor(() => expect(pending).toHaveLength(1));
    pending.shift()!(true);

    await waitFor(() => expect(view.result.current).toBe(true));
  });

  it("does not write after unmount", async () => {
    const view = await mount();
    resizeHandlers[0]();
    await waitFor(() => expect(pending).toHaveLength(1));

    view.unmount();
    // Landing an answer into an unmounted hook must be inert, not a warning.
    expect(() => pending.shift()!(true)).not.toThrow();
    expect(resizeHandlers).toHaveLength(0);
    expect(focusHandlers).toHaveLength(0);
  });
});
