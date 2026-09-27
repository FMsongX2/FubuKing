import { beforeEach, describe, expect, it } from "vitest";

import type { Comment } from "@/features/artifacts/lib/comments-api";
import type { OrgDirectory } from "@/features/organisations/lib/use-org-directory";

import { buildAnchorMap } from "../lib/comment-anchors";
import { tabCommentsFor, useChatCommentsStore } from "./chat-comments-store";

function comment(id: string, anchorId: string): Comment {
  return {
    id,
    sessionId: "as-1",
    anchorKind: "message",
    anchorId,
    parentId: null,
    authorId: "u-1",
    guestName: null,
    body: "hi",
    mentions: [],
    createdAt: "2026-09-26T00:00:00Z",
    editedAt: null,
    deletedAt: null,
    resolvedAt: null,
    resolvedBy: null,
  };
}

const actions = { post: async () => {}, resolve: async () => {}, remove: async () => {} };
const EMPTY_ORG_DIRECTORY: OrgDirectory = { byId: new Map(), currentUserId: null };

const messages = [
  {
    id: "u1",
    role: "user" as const,
    content: "q",
    toolCalls: [],
    fileChanges: [],
    plan: null,
    timestamp: "t",
  },
  {
    id: "a1",
    role: "assistant" as const,
    content: "a",
    mode: "text" as const,
    toolCalls: [],
    fileChanges: [],
    plan: null,
    timestamp: "t",
  },
];
const entries = [
  { rowId: "am-p", kind: "prompt" as const, turnSeq: 1, nativeId: null, toolName: null },
  { rowId: "am-1", kind: "response" as const, turnSeq: 1, nativeId: "a1", toolName: null },
];

describe("chat-comments-store", () => {
  beforeEach(() => useChatCommentsStore.setState({ byTab: {} }));

  it("answers undefined for a tab with no target", () => {
    const s = useChatCommentsStore.getState();
    expect(s.byTab.tab?.byChatKey.a1).toBeUndefined();
    expect(tabCommentsFor(s, "tab").commentCount).toBe(0);
  });

  it("re-keys buckets by chat id and keeps bucket identity", () => {
    const { setTarget, setAnchors, setComments } = useChatCommentsStore.getState().actions;
    setTarget("tab", { remoteProjectId: "ws", sessionId: "as-1" }, entries);
    setAnchors("tab", buildAnchorMap(messages, entries));
    const bucket = [comment("c1", "am-1")];
    setComments("tab", {
      byAnchor: { "am-1": bucket },
      session: [],
      actions,
      directory: EMPTY_ORG_DIRECTORY,
    });
    const tab = tabCommentsFor(useChatCommentsStore.getState(), "tab");
    expect(tab.byChatKey.a1).toBe(bucket);
    expect(tab.byChatKey.u1).toBeUndefined();
    expect(tab.commentCount).toBe(1);
  });

  it("a frame on one anchor leaves the other bucket's reference alone", () => {
    const { setTarget, setAnchors, setComments } = useChatCommentsStore.getState().actions;
    setTarget("tab", { remoteProjectId: "ws", sessionId: "as-1" }, entries);
    setAnchors("tab", buildAnchorMap(messages, entries));
    const a = [comment("c1", "am-1")];
    const p = [comment("c2", "am-p")];
    const rc = {
      byAnchor: { "am-1": a, "am-p": p },
      session: [],
      actions,
      directory: EMPTY_ORG_DIRECTORY,
    };
    setComments("tab", rc);
    const before = tabCommentsFor(useChatCommentsStore.getState(), "tab").byChatKey.u1;
    setComments("tab", {
      ...rc,
      byAnchor: { ...rc.byAnchor, "am-1": [...a, comment("c3", "am-1")] },
    });
    const after = tabCommentsFor(useChatCommentsStore.getState(), "tab");
    expect(after.byChatKey.u1).toBe(before);
    expect(after.byChatKey.a1).toHaveLength(2);
    expect(after.commentCount).toBe(3);
  });

  it("counts the session thread once and clears on null", () => {
    const { setTarget, setAnchors, setComments, clear } = useChatCommentsStore.getState().actions;
    setTarget("tab", { remoteProjectId: "ws", sessionId: "as-1" }, entries);
    setAnchors("tab", buildAnchorMap(messages, entries));
    setComments("tab", {
      byAnchor: {},
      session: [comment("c1", "as-1")],
      actions,
      directory: EMPTY_ORG_DIRECTORY,
    });
    expect(tabCommentsFor(useChatCommentsStore.getState(), "tab").commentCount).toBe(1);
    setComments("tab", null);
    expect(tabCommentsFor(useChatCommentsStore.getState(), "tab").commentCount).toBe(0);
    clear("tab");
    expect(useChatCommentsStore.getState().byTab.tab).toBeUndefined();
  });
});
