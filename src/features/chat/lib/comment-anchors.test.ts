import { describe, expect, it } from "vitest";

import type { ChatMessage, ToolCallDisplay } from "@/types/agent";

import { buildAnchorMap, structureKey, type AnchorEntry } from "./comment-anchors";

let n = 0;
function msg(partial: Partial<ChatMessage> & { role: ChatMessage["role"] }): ChatMessage {
  n += 1;
  return {
    id: `msg-${n}`,
    content: "",
    toolCalls: [],
    fileChanges: [],
    plan: null,
    timestamp: `2026-09-26T00:00:${String(n).padStart(2, "0")}Z`,
    ...partial,
  };
}
function tool(id: string): ToolCallDisplay {
  return {
    id,
    toolName: "Read",
    kind: null,
    arguments: {},
    result: null,
    status: "completed",
    duration: null,
  };
}
function entry(partial: Partial<AnchorEntry> & Pick<AnchorEntry, "rowId" | "kind" | "turnSeq">) {
  return { nativeId: null, toolName: null, ...partial } as AnchorEntry;
}

describe("buildAnchorMap", () => {
  it("matches live assistant ids and tool ids exactly", () => {
    const messages = [
      msg({ id: "u1", role: "user", content: "hi" }),
      msg({ id: "a1", role: "assistant", content: "hello", mode: "text" }),
      msg({ id: "a2", role: "assistant", mode: "tool", toolCalls: [tool("call-1")] }),
    ];
    const entries = [
      entry({ rowId: "am-p", kind: "prompt", turnSeq: 1, nativeId: "prompt-1-x" }),
      entry({ rowId: "am-1", kind: "response", turnSeq: 1, nativeId: "a1" }),
      entry({ rowId: "tc-1", kind: "tool_call", turnSeq: 1, nativeId: "call-1", toolName: "Read" }),
    ];
    const map = buildAnchorMap(messages, entries);
    expect(map.rowIdByChatKey.get("a1")).toEqual({ rowId: "am-1", anchorKind: "message" });
    expect(map.rowIdByChatKey.get("call-1")).toEqual({ rowId: "tc-1", anchorKind: "tool_call" });
    expect(map.rowIdByChatKey.get("u1")).toEqual({ rowId: "am-p", anchorKind: "message" });
    expect(map.chatKeyByRowId.get("tc-1")).toBe("call-1");
    expect(map.ordered.map((a) => a.id)).toEqual(["am-p", "am-1", "tc-1"]);
  });

  it("pins a reloaded exchange to its turn through a surviving tool id", () => {
    // Reloaded: message ids are re-minted, tool ids are not.
    const messages = [
      msg({ role: "user", content: "one" }),
      msg({ role: "assistant", content: "first answer", mode: "text" }),
      msg({ role: "user", content: "two" }),
      msg({ role: "assistant", mode: "tool", toolCalls: [tool("call-9")] }),
      msg({ role: "assistant", content: "second answer", mode: "text" }),
    ];
    const entries = [
      entry({ rowId: "p1", kind: "prompt", turnSeq: 4 }),
      entry({ rowId: "r1", kind: "response", turnSeq: 4, nativeId: "wire-1" }),
      entry({ rowId: "p2", kind: "prompt", turnSeq: 5 }),
      entry({ rowId: "t2", kind: "tool_call", turnSeq: 5, nativeId: "call-9" }),
      entry({ rowId: "r2", kind: "response", turnSeq: 5, nativeId: "wire-2" }),
    ];
    const map = buildAnchorMap(messages, entries);
    expect(map.rowIdByChatKey.get(messages[0].id)?.rowId).toBe("p1");
    expect(map.rowIdByChatKey.get(messages[1].id)?.rowId).toBe("r1");
    expect(map.rowIdByChatKey.get(messages[2].id)?.rowId).toBe("p2");
    expect(map.rowIdByChatKey.get("call-9")?.rowId).toBe("t2");
    expect(map.rowIdByChatKey.get(messages[4].id)?.rowId).toBe("r2");
  });

  it("aligns from the end when capture started mid-session", () => {
    const messages = [
      msg({ role: "user", content: "before capture" }),
      msg({ role: "assistant", content: "old", mode: "text" }),
      msg({ role: "user", content: "two" }),
      msg({ role: "assistant", content: "b", mode: "text" }),
      msg({ role: "user", content: "three" }),
      msg({ role: "assistant", content: "c", mode: "text" }),
    ];
    const entries = [
      entry({ rowId: "p2", kind: "prompt", turnSeq: 1 }),
      entry({ rowId: "r2", kind: "response", turnSeq: 1 }),
      entry({ rowId: "p3", kind: "prompt", turnSeq: 2 }),
      entry({ rowId: "r3", kind: "response", turnSeq: 2 }),
    ];
    const map = buildAnchorMap(messages, entries);
    expect(map.rowIdByChatKey.has(messages[0].id)).toBe(false);
    expect(map.rowIdByChatKey.has(messages[1].id)).toBe(false);
    expect(map.rowIdByChatKey.get(messages[2].id)?.rowId).toBe("p2");
    expect(map.rowIdByChatKey.get(messages[3].id)?.rowId).toBe("r2");
    expect(map.rowIdByChatKey.get(messages[4].id)?.rowId).toBe("p3");
    expect(map.rowIdByChatKey.get(messages[5].id)?.rowId).toBe("r3");
  });

  it("pairs thinking with thinking and skips empty text messages", () => {
    const messages = [
      msg({ role: "user", content: "q" }),
      msg({ role: "assistant", mode: "thinking", thinking: "hmm" }),
      msg({ role: "assistant", mode: "text", content: "" }),
      msg({ role: "assistant", mode: "text", content: "part one" }),
      msg({ role: "assistant", mode: "text", content: "part two" }),
    ];
    const entries = [
      entry({ rowId: "p", kind: "prompt", turnSeq: 1 }),
      entry({ rowId: "th", kind: "thinking", turnSeq: 1 }),
      entry({ rowId: "r1", kind: "response", turnSeq: 1 }),
      entry({ rowId: "r2", kind: "response", turnSeq: 1 }),
    ];
    const map = buildAnchorMap(messages, entries);
    expect(map.rowIdByChatKey.get(messages[1].id)?.rowId).toBe("th");
    expect(map.rowIdByChatKey.has(messages[2].id)).toBe(false);
    expect(map.rowIdByChatKey.get(messages[3].id)?.rowId).toBe("r1");
    expect(map.rowIdByChatKey.get(messages[4].id)?.rowId).toBe("r2");
  });

  it("leaves a trailing exchange with no captured turn unmatched", () => {
    const messages = [
      msg({ role: "user", content: "one" }),
      msg({ role: "assistant", content: "a", mode: "text" }),
      msg({ role: "user", content: "just sent" }),
    ];
    const entries = [
      entry({ rowId: "p1", kind: "prompt", turnSeq: 1 }),
      entry({ rowId: "r1", kind: "response", turnSeq: 1 }),
    ];
    const map = buildAnchorMap(messages, entries);
    expect(map.rowIdByChatKey.get(messages[0].id)?.rowId).toBe("p1");
    expect(map.rowIdByChatKey.has(messages[2].id)).toBe(false);
  });

  it("keys every call of a run by its own id, which is what a folded group counts", () => {
    // The pill on a folded tool sequence sums the buckets of the call ids the
    // group rendered, so a comment on the LAST call of a run has to be reachable
    // by that call's id — not only the first one's.
    const messages = [
      msg({ role: "user", content: "q" }),
      msg({ role: "assistant", mode: "tool", toolCalls: [tool("c1"), tool("c2"), tool("c3")] }),
    ];
    const entries = [
      entry({ rowId: "p", kind: "prompt", turnSeq: 1 }),
      entry({ rowId: "t1", kind: "tool_call", turnSeq: 1, nativeId: "c1" }),
      entry({ rowId: "t2", kind: "tool_call", turnSeq: 1, nativeId: "c2" }),
      entry({ rowId: "t3", kind: "tool_call", turnSeq: 1, nativeId: "c3" }),
    ];
    const map = buildAnchorMap(messages, entries);
    expect(["c1", "c2", "c3"].map((id) => map.rowIdByChatKey.get(id)?.rowId)).toEqual([
      "t1",
      "t2",
      "t3",
    ]);
  });

  it("returns the empty map for nothing", () => {
    expect(buildAnchorMap([], []).ordered).toEqual([]);
  });
});

describe("structureKey", () => {
  it("changes on a new message or tool call, not on text growth", () => {
    const a = msg({ role: "assistant", content: "he", mode: "text" });
    const k1 = structureKey([a]);
    a.content = "hello";
    expect(structureKey([a])).toBe(k1);
    expect(structureKey([a, msg({ role: "assistant", content: "x" })])).not.toBe(k1);
    expect(structureKey(undefined)).toBe("0");
  });
});

/**
 * Known gaps, pinned with `it.fails` (0.3.4 hardening report): each asserts
 * the CORRECT mapping and currently fails. When a fix lands, the `.fails`
 * flips red and should be dropped. C1 is fixed at the recorder.
 */
describe("buildAnchorMap — known gaps", () => {
  /// C1 (fixed). A retry rewinds the last turn and re-sends its prompt as a
  /// new turn. The recorder marks the rewound turn and leaves its rows out of
  /// the anchors (`atlas-checkpoint` `anchors`, `HistoryRewound` in capture),
  /// so after a reload the chat's two exchanges pair with the two live turns
  /// and nothing shifts onto the turn that was taken back.
  it("pairs a reloaded retried chat with the live turns the recorder keeps", () => {
    const messages = [
      msg({ id: "m1", role: "user", content: "one" }),
      msg({ id: "m2", role: "assistant", content: "a1", mode: "text" }),
      msg({ id: "m3", role: "user", content: "two" }),
      msg({ id: "m4", role: "assistant", content: "a2 (retried)", mode: "text" }),
    ];
    // Turn 2 was rewound: its rows are not among the entries.
    const entries = [
      entry({ rowId: "p1", kind: "prompt", turnSeq: 1, nativeId: "prompt-1-a" }),
      entry({ rowId: "r1", kind: "response", turnSeq: 1 }),
      entry({ rowId: "p2", kind: "prompt", turnSeq: 3, nativeId: "prompt-3-b" }),
      entry({ rowId: "r2", kind: "response", turnSeq: 3 }),
    ];
    const map = buildAnchorMap(messages, entries);
    expect(map.rowIdByChatKey.get("m1")?.rowId).toBe("p1");
    expect(map.rowIdByChatKey.get("m2")?.rowId).toBe("r1");
    expect(map.rowIdByChatKey.get("m3")?.rowId).toBe("p2");
    expect(map.rowIdByChatKey.get("m4")?.rowId).toBe("r2");
  });

  /// C2. Entries refresh only on resolve (turn end, capture-changed). Once a
  /// new turn's first assistant message streams in, the map rebuilds against
  /// the old entries; the trailing exchange is no longer "in flight", so every
  /// reloaded exchange shifts back by one until the next resolve.
  it.fails("stale entries while a new turn streams do not shift earlier rows", () => {
    const messages = [
      msg({ id: "m1", role: "user", content: "one" }),
      msg({ id: "m2", role: "assistant", content: "a1", mode: "text" }),
      msg({ id: "m3", role: "user", content: "two" }),
      msg({ id: "m4", role: "assistant", content: "a2", mode: "text" }),
      msg({ id: "m5", role: "user", content: "three" }),
      msg({ id: "m6", role: "assistant", content: "", thinking: "hmm", mode: "thinking" }),
    ];
    const entries = [
      entry({ rowId: "p1", kind: "prompt", turnSeq: 1 }),
      entry({ rowId: "r1", kind: "response", turnSeq: 1 }),
      entry({ rowId: "p2", kind: "prompt", turnSeq: 2 }),
      entry({ rowId: "r2", kind: "response", turnSeq: 2 }),
    ];
    const map = buildAnchorMap(messages, entries);
    expect(map.rowIdByChatKey.get("m1")?.rowId).toBe("p1");
    expect(map.rowIdByChatKey.get("m3")?.rowId).toBe("p2");
    expect(map.rowIdByChatKey.has("m5")).toBe(false);
  });

  /// Checkpoint rows are never in the chat; a comment on one is an orphan
  /// there by design (listed last in the panel). Pinned as passing.
  it("never maps a checkpoint entry onto a chat message", () => {
    const messages = [
      msg({ id: "m1", role: "user", content: "one" }),
      msg({ id: "m2", role: "assistant", content: "a1", mode: "text" }),
    ];
    const entries = [
      entry({ rowId: "p1", kind: "prompt", turnSeq: 1 }),
      entry({ rowId: "r1", kind: "response", turnSeq: 1 }),
      entry({ rowId: "cp1", kind: "checkpoint", turnSeq: 1 }),
    ];
    const map = buildAnchorMap(messages, entries);
    expect(map.chatKeyByRowId.has("cp1")).toBe(false);
    expect(map.rowIdByChatKey.get("m2")?.rowId).toBe("r1");
  });
});
