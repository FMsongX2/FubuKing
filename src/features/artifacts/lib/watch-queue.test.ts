import { beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.hoisted(() => vi.fn());
vi.mock("@tauri-apps/api/core", () => ({ invoke }));

const { queueFollow, queueUnfollow } = await import("./watch-queue");

function deferred() {
  let resolve!: () => void;
  const promise = new Promise<void>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}

/** Let every settled promise run its continuations. */
const flush = () => new Promise<void>((resolve) => setTimeout(resolve, 0));

beforeEach(() => {
  invoke.mockReset();
});

describe("watch queue", () => {
  it("runs follow, unfollow, follow strictly in call order", async () => {
    // StrictMode's double mount. If the second follow ever overtook the
    // unfollow, the refcount ended one short and the socket closed under the
    // watcher that was still mounted.
    const first = deferred();
    const second = deferred();
    const third = deferred();
    invoke
      .mockReturnValueOnce(first.promise)
      .mockReturnValueOnce(second.promise)
      .mockReturnValueOnce(third.promise);

    void queueFollow("ws_1", "ses_1");
    void queueUnfollow("ws_1", "ses_1");
    const last = queueFollow("ws_1", "ses_1");

    await flush();
    expect(invoke).toHaveBeenCalledTimes(1);
    first.resolve();
    await flush();
    expect(invoke).toHaveBeenCalledTimes(2);
    second.resolve();
    await flush();
    expect(invoke).toHaveBeenCalledTimes(3);
    third.resolve();
    await last;

    expect(invoke.mock.calls.map((c) => [c[0], c[1]])).toEqual([
      ["artifacts_cloud_follow", { projectId: "ws_1", sessionId: "ses_1" }],
      ["artifacts_cloud_unfollow", { projectId: "ws_1", sessionId: "ses_1" }],
      ["artifacts_cloud_follow", { projectId: "ws_1", sessionId: "ses_1" }],
    ]);
  });

  it("keeps going after a failed call", async () => {
    invoke.mockRejectedValueOnce(new Error("no socket")).mockResolvedValueOnce(undefined);
    await queueFollow("ws_1", "ses_1");
    await queueFollow("ws_1", "ses_2");
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(invoke.mock.calls[1]?.[1]).toEqual({ projectId: "ws_1", sessionId: "ses_2" });
  });
});
