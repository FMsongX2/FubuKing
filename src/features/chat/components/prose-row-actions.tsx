// Pin / copy / comment, under an agent response.
//
// The same bar the prompt wears (`user-row-actions.tsx`), with the same three
// constraints and the same reveal: absolutely positioned into a gap the row
// reserves (`pb-7` on the prose column, only once the row has settled), no
// subscription to the chat store, and a snap rather than a fade. Read the
// notes there before changing anything here.
//
// Left-aligned rather than right: a response starts at the column's left
// edge and the bar belongs under its first line, where the reader's eye is
// when the answer ends.

import { useCallback } from "react";
import { Pin } from "lucide-react";
import { cn } from "@/lib/utils";
import { HintGroup } from "@/ui/hint-group";
import { useChatPinsStore } from "../stores/chat-pins-store";
import { useRowHasComments } from "./chat-comment-pills";
import { ActionButton, CommentAndCopy } from "./user-row-actions";

export function ProseRowActions({
  tabId,
  messageId,
  timestamp,
  text,
  pinScopeKey,
}: {
  tabId: string;
  /** `ChatMessage.id` — the row id is `p:<messageId>`. */
  messageId: string;
  timestamp: string;
  /** The prose as shown (`row.text`) — what copy hands over and half of the
   *  pin's durable key. */
  text: string;
  pinScopeKey: string;
}) {
  const pinned = useChatPinsStore((s) =>
    (s.pins[pinScopeKey] ?? []).some((p) => p.messageId === messageId),
  );
  const discussed = useRowHasComments(tabId, messageId);

  const onPin = useCallback(() => {
    useChatPinsStore.getState().actions.toggle(pinScopeKey, {
      messageId,
      timestamp,
      text,
      at: new Date().toISOString(),
      role: "assistant",
    });
  }, [pinScopeKey, messageId, timestamp, text]);

  return (
    <HintGroup>
      <div
        className={cn(
          // Into the column's reserved `pb-7`: 8px of air, then the 20px icons.
          "absolute bottom-0 left-6 z-popover flex w-max items-center gap-0.5 pt-2",
          discussed ? "visible" : "invisible group-hover:visible focus-within:visible",
        )}
      >
        <ActionButton label="Pin response" onClick={onPin} active={pinned}>
          <Pin size={12} fill={pinned ? "currentColor" : "none"} />
        </ActionButton>
        <CommentAndCopy tabId={tabId} messageId={messageId} text={text} label="Copy response" />
      </div>
    </HintGroup>
  );
}
