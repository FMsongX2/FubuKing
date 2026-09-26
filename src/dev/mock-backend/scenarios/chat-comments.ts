// A shared chat: the prompt and the response both carry cloud comments, so the
// action bars render with the comment pill in them. The layout to review is
// the bar under each node — pill flush with the bubble's edge on the prompt,
// under the first line on the response.

import type { Scenario } from "../types";
import type { Comment } from "@/features/artifacts/lib/comments-api";
import { mockComment } from "../fixtures/artifacts";
import { text, user, t } from "../fixtures/chat";
import { setSeedTranscript } from "../fake-agent";

const transcript = [user("hello world", t(0)), text("Hey! 👋 How can I help you today?", t(1))];
const SESSION = "as-mock-chat";
const PROMPT_ROW = "am-prompt-1";
const RESPONSE_ROW = "am-response-1";
const CHECKPOINT_ROW = "cp-1";

const comments = [
  mockComment({ id: "cc_1", sessionId: SESSION, anchorId: PROMPT_ROW, body: "hello" }),
  mockComment({ id: "cc_2", sessionId: SESSION, anchorId: PROMPT_ROW, body: "again" }),
  mockComment({ id: "cc_3", sessionId: SESSION, anchorId: RESPONSE_ROW, body: "nice" }),
  mockComment({ id: "cc_4", sessionId: SESSION, anchorId: RESPONSE_ROW, body: "is it?" }),
  mockComment({
    id: "cc_5",
    sessionId: SESSION,
    anchorId: RESPONSE_ROW,
    parentId: "cc_4",
    body: "yes",
    authorId: "user_bob",
  }),
];

/** A chat-comments scenario over `rows`, with `list` answering the comments
 *  call (or throwing, for the error variant). */
function commentScenario(name: string, description: string, list: () => Comment[]): Scenario {
  return {
    name,
    description,
    init: () => setSeedTranscript(transcript),
    commands: {
      chat_comment_target: () => ({
        remoteProjectId: "rw_8c41f20b",
        sessionId: SESSION,
        entries: [
          { rowId: PROMPT_ROW, kind: "prompt", turnSeq: 1, nativeId: "prompt-1-x", toolName: null },
          // The response's native id is the wire message id, as capture records it.
          {
            rowId: RESPONSE_ROW,
            kind: "response",
            turnSeq: 1,
            nativeId: transcript[1].id,
            toolName: null,
          },
          // A checkpoint row: never in the chat, so comments on it are orphans there.
          { rowId: CHECKPOINT_ROW, kind: "checkpoint", turnSeq: 1, nativeId: null, toolName: null },
        ],
      }),
      artifacts_cloud_comments: ({ sessionId }) => {
        if (String(sessionId) !== SESSION) return { byAnchor: {}, session: [] };
        const byAnchor: Record<string, Comment[]> = {};
        for (const c of list()) (byAnchor[c.anchorId] ??= []).push(c);
        return { byAnchor, session: [] };
      },
    },
  };
}

export const chatComments = commentScenario(
  "chat-comments",
  "A shared chat with comments on the prompt and the response.",
  () => comments,
);

/** Comments whose anchor row is not in the chat: one on a row that no longer
 *  exists, one on a checkpoint. The header badge counts them, the panel lists
 *  them as "A step", the transcript shows no pill for them. */
export const chatCommentsOrphan = commentScenario(
  "chat-comments-orphan",
  "Comments on a row the chat does not have (gone, and a checkpoint) beside one on the prompt.",
  () => [
    mockComment({ id: "co_1", sessionId: SESSION, anchorId: PROMPT_ROW, body: "on the prompt" }),
    mockComment({
      id: "co_2",
      sessionId: SESSION,
      anchorId: "am-gone",
      body: "row was retried away",
    }),
    mockComment({
      id: "co_3",
      sessionId: SESSION,
      anchorId: CHECKPOINT_ROW,
      anchorKind: "checkpoint",
      body: "on a checkpoint",
    }),
  ],
);

/** Sixty comments on the response from four people, a guest among them:
 *  the pill caps at "9+" with at most three faces; the popover scrolls. */
export const chatCommentsMany = commentScenario(
  "chat-comments-many",
  "Sixty comments from four people (one a guest) on the response.",
  () =>
    Array.from({ length: 60 }, (_, i) =>
      mockComment({
        id: `cm_${i}`,
        sessionId: SESSION,
        anchorId: RESPONSE_ROW,
        body: `comment ${i + 1}`,
        authorId: i % 4 === 3 ? null : ["user_ada", "user_bob", "user_cy"][i % 4],
        guestName: i % 4 === 3 ? "Guest Reviewer" : null,
        parentId: i > 0 && i % 5 === 0 ? "cm_0" : null,
      } as Partial<Comment> & { id: string }),
    ),
);

/** A deleted root with its two replies kept: the pill counts the two replies. */
export const chatCommentsRepliesOnly = commentScenario(
  "chat-comments-replies-only",
  "A deleted comment whose two replies remain, on the prompt.",
  () => [
    mockComment({
      id: "cr_1",
      sessionId: SESSION,
      anchorId: PROMPT_ROW,
      body: "",
      deletedAt: "2026-09-20T10:05:00.000Z",
    }),
    mockComment({
      id: "cr_2",
      sessionId: SESSION,
      anchorId: PROMPT_ROW,
      parentId: "cr_1",
      body: "reply one",
    }),
    mockComment({
      id: "cr_3",
      sessionId: SESSION,
      anchorId: PROMPT_ROW,
      parentId: "cr_1",
      body: "reply two",
      authorId: "user_bob",
    }),
  ],
);

/** The comments read fails: no pills, no toast, no retry loop. */
export const chatCommentsError = commentScenario(
  "chat-comments-error",
  "The comments read fails: the chat shows no pills and no error.",
  () => {
    throw new Error("comments service unavailable");
  },
);
