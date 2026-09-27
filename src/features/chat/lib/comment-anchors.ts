/**
 * Which captured row a live chat message IS, for the purpose of comments.
 *
 * A comment on the shared Timeline hangs off a captured row id (`am-…`,
 * `tc-…`). The chat never sees those ids: its messages carry the agent's own
 * ids, and after a history reload not even those (`replaceMessages` re-mints
 * every message id; tool call ids survive). Capture keeps the agent's id
 * beside each row it writes, and this module joins the two — exactly where it
 * can, and by position where it cannot.
 *
 * # The three ways a row is found
 *
 * 1. **Tool calls, by id.** `native_call_id` is the wire `ToolCall.id`, which
 *    the chat store keeps through every reload. Exact, always.
 * 2. **Assistant text and thinking, by id.** `native_message_id` is the wire
 *    `Message.id` — the chat's id for a message streamed in this process, and
 *    not for one loaded from history.
 * 3. **Everything else, by turn.** A prompt's native id is synthesised from the
 *    turn and a content hash, so it is never a chat id. Instead the chat is cut
 *    into exchanges (a user message and the assistant run after it) and each
 *    exchange is paired with a captured turn: pinned by any id match inside it,
 *    otherwise by position **from the end**, because the newest exchanges are
 *    the live ones and the usual divergence — capture switched on mid-session
 *    — leaves the oldest exchanges without a captured turn, not the newest.
 *    Inside a paired exchange, rows and messages of the same kind pair in order.
 *
 * Empty-bodied messages are skipped when pairing by position: capture never
 * records one (`capture.rs`, `submit_pending_messages`), so a chat message with
 * no text has no row to be.
 *
 * Pure and synchronous. Runs once per structural change of the thread (a new
 * message, a resolve), never per streaming chunk.
 */

import type { AnchorKind } from "@/features/artifacts/lib/comments-api";
import type { ChatMessage } from "@/types/agent";

/** One commentable captured row, as `chat_comment_target` returns it. */
export interface AnchorEntry {
  rowId: string;
  kind: "prompt" | "response" | "thinking" | "tool_call" | "checkpoint";
  turnSeq: number;
  nativeId: string | null;
  toolName: string | null;
}

export interface AnchorHit {
  rowId: string;
  anchorKind: AnchorKind;
}

/** A row the comments panel can list, in the chat's own order. */
export interface OrderedAnchor {
  id: string;
  kind: AnchorEntry["kind"];
  toolName: string | null;
}

export interface AnchorMap {
  /** `ChatMessage.id` or `ToolCallDisplay.id` → the captured row it is. */
  rowIdByChatKey: Map<string, AnchorHit>;
  /** The reverse: captured row id → chat key. */
  chatKeyByRowId: Map<string, string>;
  /** Every matched row, in transcript order, for the panel. */
  ordered: OrderedAnchor[];
}

export const EMPTY_ANCHOR_MAP: AnchorMap = {
  rowIdByChatKey: new Map(),
  chatKeyByRowId: new Map(),
  ordered: [],
};

/**
 * A cheap fingerprint of the thread's SHAPE, for a store selector: the map
 * needs rebuilding when a message or tool call is added, not when the trailing
 * message grows by a chunk.
 */
export function structureKey(messages: readonly ChatMessage[] | undefined): string {
  if (!messages || messages.length === 0) return "0";
  const last = messages[messages.length - 1];
  return `${messages.length}:${last.id}:${last.toolCalls.length}`;
}

function anchorKindOf(kind: AnchorEntry["kind"]): AnchorKind {
  switch (kind) {
    case "tool_call":
      return "tool_call";
    case "checkpoint":
      return "checkpoint";
    default:
      return "message";
  }
}

function hasText(m: ChatMessage): boolean {
  return !!(m.content && m.content.trim());
}

function hasThinking(m: ChatMessage): boolean {
  return !!(m.thinking && m.thinking.trim());
}

interface Exchange {
  user: ChatMessage | null;
  assistant: ChatMessage[];
}

function splitExchanges(messages: readonly ChatMessage[]): Exchange[] {
  const out: Exchange[] = [];
  let current: Exchange | null = null;
  for (const m of messages) {
    if (m.role === "user") {
      current = { user: m, assistant: [] };
      out.push(current);
      continue;
    }
    // A leading assistant run — a resumed thread whose first prompt fell
    // outside the loaded window — gets an exchange of its own.
    if (!current) {
      current = { user: null, assistant: [] };
      out.push(current);
    }
    current.assistant.push(m);
  }
  return out;
}

export function buildAnchorMap(
  messages: readonly ChatMessage[],
  entries: readonly AnchorEntry[],
): AnchorMap {
  if (messages.length === 0 || entries.length === 0) return EMPTY_ANCHOR_MAP;

  const byNative = new Map<string, AnchorEntry>();
  const byTurn = new Map<number, AnchorEntry[]>();
  for (const e of entries) {
    if (e.nativeId) byNative.set(e.nativeId, e);
    const bucket = byTurn.get(e.turnSeq);
    if (bucket) bucket.push(e);
    else byTurn.set(e.turnSeq, [e]);
  }
  const turnSeqs = [...byTurn.keys()].sort((a, b) => a - b);

  const rowIdByChatKey = new Map<string, AnchorHit>();
  const chatKeyByRowId = new Map<string, string>();
  const claimedRows = new Set<string>();
  const hit = (chatKey: string, entry: AnchorEntry) => {
    if (claimedRows.has(entry.rowId) || rowIdByChatKey.has(chatKey)) return;
    claimedRows.add(entry.rowId);
    rowIdByChatKey.set(chatKey, { rowId: entry.rowId, anchorKind: anchorKindOf(entry.kind) });
    chatKeyByRowId.set(entry.rowId, chatKey);
  };

  const exchanges = splitExchanges(messages);

  // Passes 1 and 2: exact ids. Each match also pins its exchange to a turn.
  const pinnedTurn = new Map<Exchange, number>();
  const pinnedExchanges = new Set<number>();
  const pin = (ex: Exchange, turnSeq: number) => {
    if (pinnedTurn.has(ex) || pinnedExchanges.has(turnSeq)) return;
    pinnedTurn.set(ex, turnSeq);
    pinnedExchanges.add(turnSeq);
  };
  for (const ex of exchanges) {
    for (const m of ex.assistant) {
      for (const tc of m.toolCalls) {
        const e = byNative.get(tc.id);
        if (e && e.kind === "tool_call") {
          hit(tc.id, e);
          pin(ex, e.turnSeq);
        }
      }
      const e = byNative.get(m.id);
      if (e && e.kind !== "tool_call") {
        hit(m.id, e);
        pin(ex, e.turnSeq);
      }
    }
  }

  // Pass 3: pair the unpinned exchanges with the unpinned turns, newest first.
  // An exchange with neither a user message nor assistant rows carries nothing
  // to pair, so it is left out of the count.
  const freeExchanges = exchanges.filter(
    (ex) => !pinnedTurn.has(ex) && (ex.user !== null || ex.assistant.length > 0),
  );
  const freeTurns = turnSeqs.filter((seq) => !pinnedExchanges.has(seq));
  // A prompt just sent has no captured turn for a moment (the worker writes it
  // asynchronously) and no answer yet. When there are more exchanges than
  // turns, such trailing in-flight exchanges are the ones without a row — not
  // the answered exchange before them.
  while (
    freeExchanges.length > freeTurns.length &&
    freeExchanges[freeExchanges.length - 1].assistant.length === 0
  ) {
    freeExchanges.pop();
  }
  for (let i = 1; i <= Math.min(freeExchanges.length, freeTurns.length); i++) {
    const ex = freeExchanges[freeExchanges.length - i];
    const seq = freeTurns[freeTurns.length - i];
    pinnedTurn.set(ex, seq);
  }

  // Inside each paired exchange, same-kind rows pair in order. Tool calls are
  // never paired by position — an id that did not match is a row that is not
  // this call.
  for (const ex of exchanges) {
    const seq = pinnedTurn.get(ex);
    if (seq === undefined) continue;
    const rows = (byTurn.get(seq) ?? []).filter((e) => !claimedRows.has(e.rowId));
    const prompts = rows.filter((e) => e.kind === "prompt");
    const responses = rows.filter((e) => e.kind === "response");
    const thoughts = rows.filter((e) => e.kind === "thinking");

    if (ex.user && prompts.length > 0 && !rowIdByChatKey.has(ex.user.id)) {
      hit(ex.user.id, prompts[0]);
    }
    let r = 0;
    let t = 0;
    for (const m of ex.assistant) {
      if (rowIdByChatKey.has(m.id)) continue;
      if (m.mode === "thinking") {
        if (hasThinking(m) && t < thoughts.length) hit(m.id, thoughts[t++]);
      } else if (m.mode !== "tool" && hasText(m)) {
        if (r < responses.length) hit(m.id, responses[r++]);
      }
    }
  }

  // Derived view, in transcript order.
  const ordered: OrderedAnchor[] = [];
  const entryByRow = new Map(entries.map((e) => [e.rowId, e] as const));
  const push = (chatKey: string) => {
    const h = rowIdByChatKey.get(chatKey);
    if (!h) return;
    const e = entryByRow.get(h.rowId);
    if (e) ordered.push({ id: e.rowId, kind: e.kind, toolName: e.toolName });
  };
  for (const ex of exchanges) {
    if (ex.user) push(ex.user.id);
    for (const m of ex.assistant) {
      push(m.id);
      for (const tc of m.toolCalls) push(tc.id);
    }
  }

  return { rowIdByChatKey, chatKeyByRowId, ordered };
}
