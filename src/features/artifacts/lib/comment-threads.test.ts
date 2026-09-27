import { describe, expect, it } from "vitest";

import type { OrgMember } from "@/features/auth/lib/auth-api";
import type { OrgDirectory } from "@/features/organisations/lib/use-org-directory";

import {
  buildThreads,
  DEFAULT_THREAD_FILTERS,
  filterKeyForKind,
  filterThreads,
  threadAuthors,
  threadSize,
  threadTally,
  type ThreadFilters,
} from "./comment-threads";
import type { Comment } from "./comments-api";
import { DEFAULT_FILTERS, type TimelineEntry } from "../types";

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

function entry(id: string, kind: TimelineEntry["kind"] = "prompt"): TimelineEntry {
  return { id, kind, at: "2026-09-20T10:00:00.000Z", turnSeq: 0 } as TimelineEntry;
}

const ENTRIES = [entry("msg_1"), entry("tc_1", "tool_call"), entry("msg_2", "response")];

function filters(over: Partial<ThreadFilters> = {}): ThreadFilters {
  return { ...DEFAULT_THREAD_FILTERS, ...over };
}

describe("buildThreads", () => {
  it("pairs a root with its replies", () => {
    const threads = buildThreads(
      {
        msg_1: [
          comment({ id: "c1" }),
          comment({ id: "c2", parentId: "c1" }),
          comment({ id: "c3", parentId: "c1" }),
        ],
      },
      [],
      ENTRIES,
    );
    expect(threads).toHaveLength(1);
    expect(threads[0].root.id).toBe("c1");
    expect(threads[0].replies.map((r) => r.id)).toEqual(["c2", "c3"]);
  });

  it("puts the session-level thread above every node", () => {
    // It is about the whole record rather than any one step, so transcript
    // order does not apply to it.
    const threads = buildThreads(
      { msg_1: [comment({ id: "c2" })] },
      [comment({ id: "c1", anchorKind: "session", anchorId: "ses_1" })],
      ENTRIES,
    );
    expect(threads[0].entryIndex).toBe(-1);
    expect(threads[0].anchorKind).toBe("session");
  });

  it("orders node threads the way the transcript reads", () => {
    // `Object.entries` order is insertion order, which is whatever the server
    // happened to return — the panel has to impose the record's own order.
    const threads = buildThreads(
      {
        msg_2: [comment({ id: "c3", anchorId: "msg_2" })],
        msg_1: [comment({ id: "c1", anchorId: "msg_1" })],
        tc_1: [comment({ id: "c2", anchorId: "tc_1", anchorKind: "tool_call" })],
      },
      [],
      ENTRIES,
    );
    const sorted = filterThreads(threads, filters({ sort: "oldest" }), DIRECTORY);
    expect(sorted.map((t) => t.anchorId)).toEqual(["msg_1", "tc_1", "msg_2"]);
  });

  it("keeps a thread whose anchor is not in this read", () => {
    // A window that has not grown that far, or a row since removed. Sorting it
    // last is fine; dropping it would hide a real conversation.
    const threads = buildThreads({ gone: [comment({ id: "c1", anchorId: "gone" })] }, [], ENTRIES);
    expect(threads).toHaveLength(1);
    expect(threads[0].entryIndex).toBe(Number.MAX_SAFE_INTEGER);
  });

  it("survives a bucket whose root was deleted", () => {
    // The server keeps a deleted comment's row so replies keep their places, but
    // a bucket can still arrive with no parentless comment in it.
    const threads = buildThreads(
      { msg_1: [comment({ id: "c2", parentId: "gone" }), comment({ id: "c3", parentId: "gone" })] },
      [],
      ENTRIES,
    );
    expect(threads).toHaveLength(1);
    expect(threads[0].root.id).toBe("c2");
  });

  it("dates a thread by its newest comment, not its first", () => {
    // A thread replied to today is not three weeks old, and sorting it as such
    // buries the live conversation under dead ones.
    const threads = buildThreads(
      {
        msg_1: [
          comment({ id: "c1", createdAt: "2026-09-01T10:00:00.000Z" }),
          comment({ id: "c2", parentId: "c1", createdAt: "2026-09-20T10:00:00.000Z" }),
        ],
      },
      [],
      ENTRIES,
    );
    expect(threads[0].lastActivityAt).toBe("2026-09-20T10:00:00.000Z");
  });

  it("collects speakers in order and leaves out deleted ones", () => {
    const threads = buildThreads(
      {
        msg_1: [
          comment({ id: "c1", authorId: "u2" }),
          comment({ id: "c2", parentId: "c1", authorId: "u1" }),
          comment({ id: "c3", parentId: "c1", authorId: "u1" }),
          comment({ id: "c4", parentId: "c1", authorId: "u_gone", deletedAt: "then" }),
        ],
      },
      [],
      ENTRIES,
    );
    expect(threads[0].authorIds).toEqual(["u2", "u1"]);
  });
});

describe("filterThreads", () => {
  const threads = buildThreads(
    {
      msg_1: [comment({ id: "c1", body: "the retry helper looks wrong", authorId: "u1" })],
      tc_1: [
        comment({
          id: "c2",
          anchorId: "tc_1",
          anchorKind: "tool_call",
          body: "this call timed out",
          authorId: "u2",
          resolvedAt: "2026-09-21T10:00:00.000Z",
          createdAt: "2026-09-21T10:00:00.000Z",
        }),
      ],
    },
    [],
    ENTRIES,
  );

  it("hides resolved threads by default and reveals them on request", () => {
    // Resolved means done. Out of the way, not gone.
    expect(filterThreads(threads, filters(), DIRECTORY).map((t) => t.anchorId)).toEqual(["msg_1"]);
    expect(filterThreads(threads, filters({ showResolved: true }), DIRECTORY)).toHaveLength(2);
  });

  it("matches the query against comment bodies", () => {
    const found = filterThreads(
      threads,
      filters({ query: "RETRY", showResolved: true }),
      DIRECTORY,
    );
    expect(found.map((t) => t.anchorId)).toEqual(["msg_1"]);
  });

  it("matches the query against author names too", () => {
    // You remember who said it more often than what they typed.
    const found = filterThreads(
      threads,
      filters({ query: "grace", showResolved: true }),
      DIRECTORY,
    );
    expect(found.map((t) => t.anchorId)).toEqual(["tc_1"]);
  });

  it("narrows to one author", () => {
    const found = filterThreads(
      threads,
      filters({ authorId: "u2", showResolved: true }),
      DIRECTORY,
    );
    expect(found.map((t) => t.anchorId)).toEqual(["tc_1"]);
  });

  it("combines filters rather than picking one", () => {
    // Author u2's only thread is resolved, so with resolved hidden this is empty
    // — not "u2's threads" and not "unresolved threads".
    expect(filterThreads(threads, filters({ authorId: "u2" }), DIRECTORY)).toHaveLength(0);
  });

  it("sorts by newest activity, and by oldest when asked", () => {
    const newest = filterThreads(threads, filters({ showResolved: true }), DIRECTORY);
    expect(newest.map((t) => t.anchorId)).toEqual(["tc_1", "msg_1"]);
    const oldest = filterThreads(
      threads,
      filters({ showResolved: true, sort: "oldest" }),
      DIRECTORY,
    );
    expect(oldest.map((t) => t.anchorId)).toEqual(["msg_1", "tc_1"]);
  });

  it("breaks a timestamp tie with transcript order", () => {
    // Otherwise two threads from the same second swap places on every render,
    // which reads as the list flickering under an unchanged filter.
    const tied = buildThreads(
      {
        msg_2: [comment({ id: "c2", anchorId: "msg_2" })],
        msg_1: [comment({ id: "c1", anchorId: "msg_1" })],
      },
      [],
      ENTRIES,
    );
    for (const sort of ["newest", "oldest"] as const) {
      expect(filterThreads(tied, filters({ sort }), DIRECTORY).map((t) => t.anchorId)).toEqual([
        "msg_1",
        "msg_2",
      ]);
    }
  });

  it("does not mutate the list it was given", () => {
    const before = threads.map((t) => t.anchorId);
    filterThreads(threads, filters({ showResolved: true, sort: "oldest" }), DIRECTORY);
    expect(threads.map((t) => t.anchorId)).toEqual(before);
  });
});

describe("filterKeyForKind", () => {
  it("maps every entry kind to the filter that reveals it", () => {
    // The jump resolver reads this to un-hide a target. It used to enable
    // `checkpoints` whatever the target was, which was invisible while the only
    // jump sources were Checkpoints and chat citations.
    expect(filterKeyForKind("prompt")).toBe("prompts");
    expect(filterKeyForKind("response")).toBe("responses");
    expect(filterKeyForKind("thinking")).toBe("thinking");
    expect(filterKeyForKind("tool_call")).toBe("toolCalls");
    expect(filterKeyForKind("checkpoint")).toBe("checkpoints");
  });

  it("does not send a thinking entry to the Checkpoint filter", () => {
    // The specific regression. `thinking` is off by default and a comment can
    // anchor to one, so the old behaviour made that click do nothing at all.
    expect(filterKeyForKind("thinking")).not.toBe("checkpoints");
  });

  it("names a real key of TimelineFilters for every kind", () => {
    // A typo here would compile — `keyof` is satisfied by any valid key — and
    // then silently enable the wrong filter.
    const keys = Object.keys(DEFAULT_FILTERS);
    for (const kind of ["prompt", "response", "thinking", "tool_call", "checkpoint"] as const) {
      expect(keys).toContain(filterKeyForKind(kind));
    }
  });
});

describe("threadAuthors / threadSize", () => {
  it("lists every speaker once, across threads", () => {
    const threads = buildThreads(
      {
        msg_1: [comment({ id: "c1", authorId: "u1" })],
        tc_1: [comment({ id: "c2", anchorId: "tc_1", authorId: "u2" })],
        msg_2: [comment({ id: "c3", anchorId: "msg_2", authorId: "u1" })],
      },
      [],
      ENTRIES,
    );
    expect(threadAuthors(threads).sort()).toEqual(["u1", "u2"]);
  });

  it("counts what is actually shown", () => {
    // A tombstone is not a comment; the count beside a thread has to agree with
    // what the popover renders.
    const [thread] = buildThreads(
      {
        msg_1: [
          comment({ id: "c1" }),
          comment({ id: "c2", parentId: "c1" }),
          comment({ id: "c3", parentId: "c1", deletedAt: "then" }),
        ],
      },
      [],
      ENTRIES,
    );
    expect(threadSize(thread)).toBe(2);
  });
});

describe("threadTally", () => {
  it("counts top-level comments and replies apart, skipping tombstones", () => {
    const base = {
      sessionId: "s",
      anchorKind: "message" as const,
      anchorId: "a",
      authorId: "u",
      guestName: null,
      mentions: [],
      editedAt: null,
      deletedAt: null,
      resolvedAt: null,
      resolvedBy: null,
    };
    const threads = buildThreads(
      {
        a: [
          { ...base, id: "c1", parentId: null, body: "one", createdAt: "1" },
          { ...base, id: "c2", parentId: null, body: "two", createdAt: "2" },
          { ...base, id: "c3", parentId: "c2", body: "ok", createdAt: "3" },
          { ...base, id: "c4", parentId: "c1", body: null, createdAt: "4", deletedAt: "5" },
        ],
      },
      [],
      [{ id: "a" }],
    );
    expect(threadTally(threads[0])).toEqual({ comments: 2, replies: 1 });
  });
});
