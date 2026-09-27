import { beforeEach, describe, expect, it } from "vitest";
import { useEditorStore } from "./editor-store";

const actions = () => useEditorStore.getState().actions;

beforeEach(() => {
  useEditorStore.setState({ buffers: {}, activeBufferPath: null, pendingReveals: {} });
});

describe("pending reveals", () => {
  it("records a reveal for a path", () => {
    actions().requestReveal("/a.ts", { line: 10 });
    expect(useEditorStore.getState().pendingReveals["/a.ts"]).toMatchObject({ line: 10 });
  });

  /// The panel's effect keys on the nonce: asking for the same line twice
  /// must fire twice, or "go back to line 10" silently does nothing.
  it("gives a repeated reveal of the same line a new nonce", () => {
    actions().requestReveal("/a.ts", { line: 10 });
    const first = useEditorStore.getState().pendingReveals["/a.ts"].nonce;
    actions().requestReveal("/a.ts", { line: 10 });
    expect(useEditorStore.getState().pendingReveals["/a.ts"].nonce).not.toBe(first);
  });

  it("consuming the current reveal clears it", () => {
    actions().requestReveal("/a.ts", { line: 10 });
    const { nonce } = useEditorStore.getState().pendingReveals["/a.ts"];
    actions().consumeReveal("/a.ts", nonce);
    expect(useEditorStore.getState().pendingReveals["/a.ts"]).toBeUndefined();
  });

  /// A reveal requested while the panel was applying the previous one must
  /// survive that previous one's consume.
  it("consuming a stale reveal keeps the newer one", () => {
    actions().requestReveal("/a.ts", { line: 10 });
    const stale = useEditorStore.getState().pendingReveals["/a.ts"].nonce;
    actions().requestReveal("/a.ts", { line: 20 });
    actions().consumeReveal("/a.ts", stale);
    expect(useEditorStore.getState().pendingReveals["/a.ts"]).toMatchObject({ line: 20 });
  });

  it("closing a buffer drops its pending reveal", () => {
    actions().openBuffer("/a.ts", "x");
    actions().requestReveal("/a.ts", { line: 3 });
    actions().closeBuffer("/a.ts");
    expect(useEditorStore.getState().pendingReveals["/a.ts"]).toBeUndefined();
  });
});
