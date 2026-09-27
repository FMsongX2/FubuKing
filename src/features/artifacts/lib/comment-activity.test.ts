import { describe, expect, it } from "vitest";

import type { OrgMember } from "@/features/auth/lib/auth-api";
import type { OrgDirectory } from "@/features/organisations/lib/use-org-directory";

import { commentActivity } from "./comment-activity";
import type { Comment } from "./comments-api";

function member(userId: string, name: string): OrgMember {
  return {
    id: `mem_${userId}`,
    userId,
    name,
    email: `${userId}@example.invalid`,
    role: "member",
    createdAt: null,
    avatarPath: null,
  } as OrgMember;
}

const DIRECTORY: OrgDirectory = {
  byId: new Map([
    ["u1", member("u1", "Ada Lovelace")],
    ["u2", member("u2", "Grace Hopper")],
  ]),
  currentUserId: "u1",
};

function comment(over: Partial<Comment> & { id: string }): Comment {
  return {
    sessionId: "ses_1",
    anchorKind: "message",
    anchorId: "msg_1",
    parentId: null,
    authorId: "u1",
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

describe("commentActivity", () => {
  it("is empty for a node nobody has commented on", () => {
    expect(commentActivity(undefined, DIRECTORY)).toEqual([]);
    expect(commentActivity([], DIRECTORY)).toEqual([]);
  });

  it("reports a root as a comment, with no target", () => {
    const [line] = commentActivity([comment({ id: "c1" })], DIRECTORY);
    expect(line.isReply).toBe(false);
    expect(line.actorName).toBe("Ada Lovelace");
    expect(line.targetName).toBeNull();
  });

  it("names who was replied to", () => {
    const lines = commentActivity(
      [
        comment({ id: "c1", authorId: "u1" }),
        comment({ id: "c2", parentId: "c1", authorId: "u2" }),
      ],
      DIRECTORY,
    );
    expect(lines[1].isReply).toBe(true);
    expect(lines[1].actorName).toBe("Grace Hopper");
    expect(lines[1].targetName).toBe("Ada Lovelace");
    expect(lines[1].self).toBe(false);
  });

  it("flags a reply to your own comment", () => {
    // "Ada replied to Ada's comment" is the same name twice in one sentence;
    // the caller says "their own" instead.
    const lines = commentActivity(
      [
        comment({ id: "c1", authorId: "u1" }),
        comment({ id: "c2", parentId: "c1", authorId: "u1" }),
      ],
      DIRECTORY,
    );
    expect(lines[1].self).toBe(true);
  });

  it("reads oldest first", () => {
    // A log under the node, and a conversation runs forwards.
    const lines = commentActivity(
      [
        comment({ id: "late", createdAt: "2026-09-21T10:00:00.000Z" }),
        comment({ id: "early", createdAt: "2026-09-20T10:00:00.000Z" }),
      ],
      DIRECTORY,
    );
    expect(lines.map((l) => l.id)).toEqual(["early", "late"]);
  });

  it("produces no line for a deleted comment", () => {
    // Its tombstone stays in the thread so replies keep their places, but
    // "X commented on this" pointing at something unreadable is a claim the UI
    // cannot back up.
    const lines = commentActivity(
      [
        comment({ id: "c1", deletedAt: "2026-09-20T11:00:00.000Z" }),
        comment({ id: "c2", authorId: "u2" }),
      ],
      DIRECTORY,
    );
    expect(lines.map((l) => l.id)).toEqual(["c2"]);
  });

  it("still names the target of a reply to a deleted comment", () => {
    // The parent's row survives deletion, so who was replied to is still known
    // even though what they said is gone.
    const lines = commentActivity(
      [
        comment({ id: "c1", authorId: "u1", deletedAt: "then" }),
        comment({ id: "c2", parentId: "c1", authorId: "u2" }),
      ],
      DIRECTORY,
    );
    expect(lines).toHaveLength(1);
    expect(lines[0].targetName).toBe("Ada Lovelace");
  });

  it("says a reply is a reply even when the parent is not in this bucket", () => {
    // Defensive: a target the client never received should not silently demote
    // the line to "commented on this", which would be the wrong event.
    const [line] = commentActivity([comment({ id: "c2", parentId: "gone" })], DIRECTORY);
    expect(line.isReply).toBe(true);
    expect(line.targetName).toBeNull();
    expect(line.self).toBe(false);
  });

  it("never prints a raw id for someone the roster cannot name", () => {
    const [line] = commentActivity([comment({ id: "c1", authorId: "u_gone" })], DIRECTORY);
    expect(line.actorName).toBe("A member");
  });

  it("renders a guest as a guest, not as a member who shares their id", () => {
    const [line] = commentActivity(
      [comment({ id: "c1", authorId: "u1", guestName: "Someone outside" })],
      DIRECTORY,
    );
    expect(line.actorName).toBe("Someone outside");
  });
});
