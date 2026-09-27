/**
 * Every discussion in a Session, as one list.
 *
 * The per-node comment button is fine once you have found the node, but finding
 * it is the problem: a Session runs to hundreds of entries and a thread
 * announces itself only as a small badge on a row you happen to scroll past.
 * This is the flattening behind the panel that lists them all.
 *
 * Pure, and separate from the panel, because the ordering and the filters are
 * the part worth pinning down: a list that quietly drops a thread is worse than
 * no list, and that is not something a component test would catch.
 */

import type { OrgDirectory } from "@/features/organisations/lib/use-org-directory";

import type { AnchorKind, Comment } from "./comments-api";
import type { TimelineEntry, TimelineFilters } from "../types";

/** One discussion: its opening comment and the replies under it. */
export interface CommentThread {
  /** The entry's `rowId`, or the Session id for a session-level thread. */
  anchorId: string;
  anchorKind: AnchorKind;
  root: Comment;
  replies: Comment[];
  /** Roots only carry resolution; lifted here so filters do not re-derive it. */
  resolved: boolean;
  /** Everyone who has spoken, in the order they first did. Drives the faces. */
  authorIds: string[];
  /** Newest activity anywhere in the thread — what "sort by date" means. A
   *  thread with a reply from today is not three weeks old. */
  lastActivityAt: string;
  /** Position in the transcript; `-1` for a session-level thread, which sorts
   *  above every node. */
  entryIndex: number;
}

export type ThreadSort = "newest" | "oldest";

export interface ThreadFilters {
  /** Matched against comment bodies and author names, case-insensitively. */
  query: string;
  /** `null` for everyone. */
  authorId: string | null;
  /** Resolved threads are done; they are out of the way unless asked for. */
  showResolved: boolean;
  sort: ThreadSort;
}

export const DEFAULT_THREAD_FILTERS: ThreadFilters = {
  query: "",
  authorId: null,
  showResolved: false,
  sort: "newest",
};

/**
 * Fold the per-anchor buckets into threads.
 *
 * `byAnchor` is keyed by the entry a comment hangs off and holds roots and
 * replies together; this is the same partition the popover does, done once for
 * the whole Session. A bucket with only replies — possible if a root was
 * deleted and the server kept its children — still produces a thread, using the
 * earliest reply as the opener, because the conversation is real even though
 * the comment that started it is gone.
 */
export function buildThreads(
  byAnchor: Record<string, Comment[]>,
  session: Comment[],
  /** The rows in transcript order; only their ids are read. */
  entries: ReadonlyArray<Pick<TimelineEntry, "id">>,
): CommentThread[] {
  const indexOf = new Map(entries.map((entry, i) => [entry.id, i] as const));
  const out: CommentThread[] = [];

  const push = (anchorId: string, comments: Comment[], entryIndex: number) => {
    const thread = toThread(anchorId, comments, entryIndex);
    if (thread) out.push(thread);
  };

  // Session-level first, at `-1`: it is about the whole record rather than any
  // one step, so it has no place in transcript order.
  if (session.length > 0) push(session[0].sessionId, session, -1);

  for (const [anchorId, comments] of Object.entries(byAnchor)) {
    // An anchor whose entry is not in this read — a window that has not grown
    // that far, or a row since removed — sorts to the end rather than
    // disappearing. A thread the panel cannot place is still a thread.
    push(anchorId, comments, indexOf.get(anchorId) ?? Number.MAX_SAFE_INTEGER);
  }

  return out;
}

function toThread(anchorId: string, comments: Comment[], entryIndex: number): CommentThread | null {
  if (comments.length === 0) return null;

  const root = comments.find((c) => !c.parentId) ?? comments[0];
  const replies = comments.filter((c) => c.id !== root.id);

  const authorIds: string[] = [];
  for (const comment of comments) {
    // A deleted comment keeps its place in the thread but its author is no
    // longer part of the visible conversation — same rule as the faces on the
    // per-node button.
    if (comment.deletedAt) continue;
    if (!authorIds.includes(comment.authorId)) authorIds.push(comment.authorId);
  }

  let lastActivityAt = root.createdAt;
  for (const comment of comments) {
    if (comment.createdAt > lastActivityAt) lastActivityAt = comment.createdAt;
  }

  return {
    anchorId,
    anchorKind: root.anchorKind,
    root,
    replies,
    resolved: root.resolvedAt !== null,
    authorIds,
    lastActivityAt,
    entryIndex,
  };
}

/**
 * Narrow and order the list.
 *
 * Every filter reads data already in memory, so this runs on each keystroke
 * without a round trip. The sort is applied last and is stable within a
 * timestamp, so two threads from the same second keep transcript order rather
 * than shuffling as the user types.
 */
export function filterThreads(
  threads: CommentThread[],
  filters: ThreadFilters,
  directory: OrgDirectory,
): CommentThread[] {
  const query = filters.query.trim().toLowerCase();

  const kept = threads.filter((thread) => {
    if (!filters.showResolved && thread.resolved) return false;
    if (filters.authorId && !thread.authorIds.includes(filters.authorId)) return false;
    if (query && !matches(thread, query, directory)) return false;
    return true;
  });

  // Transcript order first, so the date sort below only has to break ties —
  // which is what keeps the list from reshuffling under an unchanged filter.
  kept.sort((a, b) => a.entryIndex - b.entryIndex);
  kept.sort((a, b) =>
    filters.sort === "newest"
      ? b.lastActivityAt.localeCompare(a.lastActivityAt)
      : a.lastActivityAt.localeCompare(b.lastActivityAt),
  );
  return kept;
}

/** Does any comment in the thread match, by body or by who wrote it? */
function matches(thread: CommentThread, query: string, directory: OrgDirectory): boolean {
  for (const comment of [thread.root, ...thread.replies]) {
    if (comment.body?.toLowerCase().includes(query)) return true;
    const name = directory.byId.get(comment.authorId)?.name;
    if (name?.toLowerCase().includes(query)) return true;
  }
  return false;
}

/** Everyone who has commented anywhere in the Session, for the author picker. */
export function threadAuthors(threads: CommentThread[]): string[] {
  const seen = new Set<string>();
  for (const thread of threads) for (const id of thread.authorIds) seen.add(id);
  return [...seen];
}

/** How many comments the thread actually shows — deleted rows are tombstones. */
export function threadSize(thread: CommentThread): number {
  return [thread.root, ...thread.replies].filter((c) => !c.deletedAt).length;
}

/**
 * Top-level comments and replies, counted apart.
 *
 * One anchor can carry several top-level comments (two people each opening a
 * thread on the same response), and `replies` holds everything that is not
 * THE root — so "N replies" over-counted: a second top-level comment is not a
 * reply to the first. The panel says "2 comments · 1 reply", which is what the
 * popover shows.
 */
export function threadTally(thread: CommentThread): { comments: number; replies: number } {
  let comments = 0;
  let replies = 0;
  for (const c of [thread.root, ...thread.replies]) {
    if (c.deletedAt) continue;
    if (c.parentId) replies += 1;
    else comments += 1;
  }
  return { comments, replies };
}

/**
 * Which filter has to be on for an entry of this kind to be rendered.
 *
 * Used by the jump resolver: a jump to an entry the filters are hiding has to
 * reveal *that kind*, and it used to reveal Checkpoints and nothing else. That
 * was invisible while the only jump targets were Checkpoints and citation
 * chips; the comments panel can address any node, and `thinking` is off by
 * default — so a comment on a thinking entry was a click that did nothing at
 * all, forever, with no error.
 */
export function filterKeyForKind(kind: TimelineEntry["kind"]): keyof TimelineFilters {
  switch (kind) {
    case "prompt":
      return "prompts";
    case "response":
      return "responses";
    case "thinking":
      return "thinking";
    case "tool_call":
      return "toolCalls";
    case "checkpoint":
      return "checkpoints";
  }
}
