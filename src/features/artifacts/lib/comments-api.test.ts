import { describe, expect, it } from "vitest";

import {
  anchorKindFor,
  applyComment,
  EMPTY_THREADS,
  visibleCount,
  type Comment,
} from "./comments-api";

function comment(over: Partial<Comment> & { id: string }): Comment {
  return {
    sessionId: "ses_1",
    anchorKind: "message",
    anchorId: "msg_1",
    parentId: null,
    authorId: "user_ada",
    guestName: null,
    body: "hello",
    mentions: [],
    createdAt: "2026-09-20T10:00:00.000Z",
    editedAt: null,
    deletedAt: null,
    resolvedAt: null,
    resolvedBy: null,
    ...over,
  };
}

describe("anchorKindFor", () => {
  it("maps the three text kinds onto one anchor", () => {
    // Prompts, responses and thinking are all `agent_message` rows on the wire.
    // Sending `prompt` as an anchor kind is a 422 at the door.
    expect(anchorKindFor("prompt")).toBe("message");
    expect(anchorKindFor("response")).toBe("message");
    expect(anchorKindFor("thinking")).toBe("message");
  });

  it("keeps tool calls and checkpoints on their own anchors", () => {
    expect(anchorKindFor("tool_call")).toBe("tool_call");
    expect(anchorKindFor("checkpoint")).toBe("checkpoint");
  });
});

describe("applyComment", () => {
  it("appends a new comment to its anchor", () => {
    const next = applyComment(EMPTY_THREADS, comment({ id: "c1" }));
    expect(next.byAnchor["msg_1"]).toHaveLength(1);
    expect(next.session).toHaveLength(0);
  });

  it("replaces in place rather than appending a second copy", () => {
    // The realtime channel sends one frame shape for post, edit, resolve and
    // delete. Appending on an edit would show the comment twice.
    const first = applyComment(EMPTY_THREADS, comment({ id: "c1", body: "typo" }));
    const second = applyComment(first, comment({ id: "c2" }));
    const edited = applyComment(second, comment({ id: "c1", body: "fixed", editedAt: "later" }));

    expect(edited.byAnchor["msg_1"]).toHaveLength(2);
    expect(edited.byAnchor["msg_1"][0].body).toBe("fixed");
    // And keeps its position: the server answers oldest-first and an edit does
    // not move a comment to the bottom of the thread.
    expect(edited.byAnchor["msg_1"][1].id).toBe("c2");
  });

  it("handles a deletion, which arrives as a null body on an existing row", () => {
    const posted = applyComment(EMPTY_THREADS, comment({ id: "c1" }));
    const deleted = applyComment(
      posted,
      comment({ id: "c1", body: null, deletedAt: "2026-09-20T10:05:00.000Z" }),
    );
    expect(deleted.byAnchor["msg_1"]).toHaveLength(1);
    expect(deleted.byAnchor["msg_1"][0].body).toBeNull();
  });

  it("keeps session comments out of the anchor buckets", () => {
    // A session-level comment carries the Session id as its anchor. Bucketed by
    // anchor id alone it would land on whatever row happened to share that id.
    const next = applyComment(
      EMPTY_THREADS,
      comment({ id: "c1", anchorKind: "session", anchorId: "ses_1" }),
    );
    expect(next.session).toHaveLength(1);
    expect(next.byAnchor).toEqual({});
  });

  it("does not mutate the set it was given", () => {
    const before = applyComment(EMPTY_THREADS, comment({ id: "c1" }));
    const snapshot = before.byAnchor["msg_1"].length;
    applyComment(before, comment({ id: "c2" }));
    expect(before.byAnchor["msg_1"]).toHaveLength(snapshot);
  });
});

describe("visibleCount", () => {
  it("counts a whole thread, roots and replies alike", () => {
    // The count is what the node's button shows, and the web shows the thread
    // length. There is no count endpoint to disagree with.
    expect(
      visibleCount([
        comment({ id: "c1" }),
        comment({ id: "c2", parentId: "c1" }),
        comment({ id: "c3", parentId: "c1" }),
      ]),
    ).toBe(3);
  });

  it("excludes deleted rows but keeps their replies", () => {
    // A thread whose opener was removed is still a conversation; showing `0`
    // beside visible replies would read as a bug.
    expect(
      visibleCount([
        comment({ id: "c1", body: null, deletedAt: "then" }),
        comment({ id: "c2", parentId: "c1" }),
      ]),
    ).toBe(1);
  });

  it("is zero for an anchor nobody has commented on", () => {
    expect(visibleCount(undefined)).toBe(0);
    expect(visibleCount([])).toBe(0);
  });
});
