/**
 * The thread list for a live chat: the Timeline's comments panel in the
 * right-hand overlay slot the Bash and Plans panels use.
 *
 * Navigation goes through the transcript's own jump (`atlas:chat-jump`),
 * addressed by message index as every other jump is, plus the row ids to
 * highlight once it lands. A tool-call thread inside a folded "Worked" section
 * lands on the header when the row is not in the DOM.
 */

import { useCallback, useEffect, useMemo, useRef } from "react";

import { useLayoutStore } from "@/features/layout/stores/layout-store";
import {
  CommentsPanelBase,
  type PanelAnchor,
} from "@/features/artifacts/components/session-comments-panel";
import type { RowComments } from "@/features/artifacts/components/session-detail";

import { useChatCommentsStore } from "../stores/chat-comments-store";
import { useChatStore } from "../stores/chat-store";

export interface ChatJumpDetail {
  index: number;
  /** Rows to highlight on arrival, first one found wins. */
  highlightRowIds?: string[];
}

export function ChatCommentsPanel({ tabId, onClose }: { tabId: string; onClose: () => void }) {
  const tab = useChatCommentsStore((s) => s.byTab[tabId]);
  const bashPanel = useLayoutStore.use.bashPanel();
  const { setBashPanelWidth } = useLayoutStore.use.actions();

  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        e.preventDefault();
        onClose();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [onClose]);

  const resizeStartXRef = useRef<number | null>(null);
  const resizeStartWidthRef = useRef<number>(0);
  const onResizeStart = useCallback(
    (e: React.MouseEvent) => {
      e.preventDefault();
      resizeStartXRef.current = e.clientX;
      resizeStartWidthRef.current = bashPanel.width;
      const onMove = (ev: MouseEvent) => {
        if (resizeStartXRef.current === null) return;
        setBashPanelWidth(resizeStartWidthRef.current + (resizeStartXRef.current - ev.clientX));
      };
      const onUp = () => {
        resizeStartXRef.current = null;
        window.removeEventListener("mousemove", onMove);
        window.removeEventListener("mouseup", onUp);
      };
      window.addEventListener("mousemove", onMove);
      window.addEventListener("mouseup", onUp);
    },
    [bashPanel.width, setBashPanelWidth],
  );

  const anchors = useMemo<PanelAnchor[]>(
    () =>
      (tab?.anchors.ordered ?? []).map((a) => ({ id: a.id, kind: a.kind, toolName: a.toolName })),
    [tab?.anchors],
  );
  const comments = useMemo<RowComments | null>(
    () =>
      tab?.actions && tab.directory
        ? {
            byAnchor: tab.byAnchor,
            session: tab.session,
            actions: tab.actions,
            directory: tab.directory,
          }
        : null,
    [tab?.byAnchor, tab?.session, tab?.actions, tab?.directory],
  );

  const onJump = useCallback(
    (anchorId: string) => {
      const state = useChatCommentsStore.getState().byTab[tabId];
      const chatKey = state?.anchors.chatKeyByRowId.get(anchorId);
      if (!chatKey) return;
      const messages = useChatStore.getState().sessions[tabId]?.messages ?? [];
      // The message this key is, or the one holding the tool call.
      let index = messages.findIndex((m) => m.id === chatKey);
      let rowId: string;
      if (index >= 0) {
        const m = messages[index];
        rowId =
          m.role === "user" ? `u:${m.id}` : m.mode === "thinking" ? `th:${m.id}` : `p:${m.id}`;
      } else {
        index = messages.findIndex((m) => m.toolCalls.some((tc) => tc.id === chatKey));
        if (index < 0) return;
        rowId = `mk:${chatKey}`;
      }
      // Jumps address a TURN by its first message; an assistant run is one
      // turn, so walk back to where the run starts.
      let start = index;
      if (messages[index].role !== "user") {
        while (start > 0 && messages[start - 1].role !== "user") start -= 1;
      }
      const turnId = `t:${messages[start].id}`;
      window.dispatchEvent(
        new CustomEvent<ChatJumpDetail>("atlas:chat-jump", {
          detail: { index: start, highlightRowIds: [rowId, `wk:${turnId}`] },
        }),
      );
    },
    [tabId],
  );

  if (!comments) return null;

  return (
    <>
      <div
        className="absolute inset-0 z-20 scrim-soft animate-fade-in"
        onClick={onClose}
        aria-hidden
      />
      <div
        style={{ width: bashPanel.width }}
        className="absolute right-0 top-0 bottom-0 z-30 flex flex-col border-l border-[var(--border)] bg-[var(--sidebar)] shadow-md animate-slide-in-right"
      >
        <div
          onMouseDown={onResizeStart}
          className="absolute left-0 top-0 bottom-0 z-10 w-1 cursor-col-resize hover:bg-[var(--atlas-border-strong)]"
          aria-hidden
        />
        <CommentsPanelBase
          anchors={anchors}
          comments={comments}
          onJump={onJump}
          onClose={onClose}
        />
      </div>
    </>
  );
}
