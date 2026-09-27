/**
 * What happened on a node, as a sentence.
 *
 * A comment count says a discussion exists; it does not say a colleague replied
 * to you twenty minutes ago. Linear puts that under the thing it happened to,
 * in plain words, and this is the same idea for a timeline node — the record
 * shows the work, and the lines below it show the conversation about the work.
 *
 * Pure, and separate from the row, because the phrasing is the part with rules
 * in it: who replied to whom, what to say when the target is gone, and what to
 * do about a comment that no longer exists.
 */

import type { OrgDirectory } from "@/features/organisations/lib/use-org-directory";

import type { Comment } from "./comments-api";

export interface CommentActivity {
  /** The comment this line is about. */
  id: string;
  authorId: string;
  actorName: string;
  /** Whose comment was replied to. `null` on a root, or when the parent is
   *  gone — a reply to something deleted is still a reply. */
  targetName: string | null;
  /** A reply to the actor's own comment; "their own" reads better than the
   *  same name twice in one sentence. */
  self: boolean;
  isReply: boolean;
  at: string;
}

/**
 * The activity on one anchor, oldest first.
 *
 * Chronological rather than newest-first: this reads as a log under the node,
 * and a conversation runs forwards.
 *
 * **Deleted comments produce no line.** The tombstone in the thread exists so
 * replies keep their places, but "X commented on this" pointing at something
 * nobody can read is a claim the UI cannot back up.
 */
export function commentActivity(
  comments: Comment[] | undefined,
  directory: OrgDirectory,
): CommentActivity[] {
  if (!comments || comments.length === 0) return [];

  const nameOf = (comment: Comment): string =>
    comment.guestName ?? directory.byId.get(comment.authorId)?.name ?? "A member";

  const byId = new Map(comments.map((c) => [c.id, c] as const));

  return comments
    .filter((comment) => !comment.deletedAt)
    .map((comment) => {
      const parent = comment.parentId ? byId.get(comment.parentId) : undefined;
      return {
        id: comment.id,
        authorId: comment.authorId,
        actorName: nameOf(comment),
        // A parent that was deleted still has a row, and its author is still
        // who was replied to — so the name survives the deletion.
        targetName: parent ? nameOf(parent) : null,
        self: parent ? parent.authorId === comment.authorId : false,
        isReply: comment.parentId !== null,
        at: comment.createdAt,
      };
    })
    .sort((a, b) => a.at.localeCompare(b.at));
}
