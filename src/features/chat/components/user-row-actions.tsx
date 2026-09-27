// Retry / pin / edit / copy, under a user message.
//
// The "Show more" toggle is NOT here — it lives in flow above this bar
// (`ExpandToggle` in `transcript-rows.tsx`) because it is the only signal that
// a bubble is truncated at all, and a hover-only truncation marker means a
// clamped prompt reads as a complete one.
//
// Three constraints shaped this, and each one rules out the obvious approach:
//
//  1. **House rule 1 — nothing grows on hover.** The bar cannot be in flow: a
//     row that gets taller on hover reflows every row below it, mid-scroll.
//     So it is absolutely positioned into the `pb-7` gap the user column
//     reserves between one exchange and the next, and reserves no space of
//     its own. `.atlas-row` sets `contain: layout style` and deliberately NOT
//     `paint` (`globals.css`), so the bar's own bottom padding is free to
//     overhang that gap without being clipped — 28px of `pb-7` holds the
//     8px top pad and the 20px icons, which is everything that draws. That
//     gap is reserved whether or not the bar is showing, so revealing it can
//     never move anything.
//
//  2. **House rule 3 — rows never subscribe to a store.** The actions read
//     what they need through `getState()` at click time. Nothing here holds a
//     subscription, so nothing here re-renders on a streaming frame.
//
//  3. **The transcript is not virtualized** (`transcript.tsx:1`). It renders a
//     growing WINDOW — `rows.slice(safeStart)` — so this is mounted for every
//     user message currently inside it, and the window only ever grows as the
//     reader scrolls back. Rows are keyed by `row.id`, so growth prepends
//     without remounting what is already there. The DOM cost is a few buttons
//     per windowed user row, which is nothing at scroll time.
//
// # The reveal snaps. It does not animate.
//
// The bar is hidden until its row is hovered, via `group-hover` against the
// row wrapper's `group` class — a CSS hover, never a JS one. A JS hover state
// would fire a `setState` for every bubble the pointer crosses during a fast
// flick, which is precisely the work the transcript is built to avoid.
//
// What it must NOT do is transition. It used to fade in over 100ms, and a
// running opacity transition is a compositing layer in WebKit: rows passing
// under a resting pointer mid-fling each took one, several times a second, in
// the frames the tile deadline can least afford. `visibility` toggling
// instantly has nothing to interpolate and never promotes anything. Do not
// reintroduce `transition-opacity`, a fade, or an enter delay here.
//
// `visibility` rather than `opacity-0` for a second reason: an `opacity-0`
// element is still hit-testable, so the old bar could be clicked while
// invisible. And `focus-within` is not decoration — without it keyboard users
// would tab into controls they cannot see.
//
// The hover itself is still not free: `group-hover:` compiles to
// `:is(:where(.group):hover *)`, so each row the pointer crosses invalidates
// style for its whole subtree. That is what the transcript's fling
// hover-suspension is for (`use-transcript-scroll.ts`, `data-scroll-hot`) —
// it makes the content inert for the duration of a gesture so none of this
// fires while scrolling. Keep the two together; neither is sufficient alone.
//
// # Why the copy button jittered
//
// The buttons carried Tailwind's `transition-colors`, which animates `fill`
// and `stroke` as well as `color` — so hovering an icon repainted its SVG
// every frame of the transition, the "icon jitter" the global rule at
// `globals.css` ("only background-color") exists to prevent. The buttons now
// inherit that rule and transition nothing else. Labels are also constant
// (`title` used to flip to "Copied", and macOS re-anchors the native tooltip
// when it changes under the pointer); the copied state rides on the icon.

import { useCallback, useEffect, useRef, useState } from "react";
import { CopyGlyph } from "@/ui/animated-icon";
import { CornerUpRight, Pin, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { copyText } from "@/lib/clipboard";
import { HintGroup, HintItem } from "@/ui/hint-group";
import { retryLastTurn } from "../lib/retry-turn";
import { useChatPinsStore } from "../stores/chat-pins-store";
import { ActionCluster } from "@/features/artifacts/components/action-cluster";
import { CommentButton } from "@/features/artifacts/components/comment-thread";
import { visibleCount } from "@/features/artifacts/lib/comments-api";
import {
  useAnchorHit,
  useCommentActions,
  useCommentBucket,
  useCommentDirectory,
} from "../stores/chat-comments-store";
import { useRowHasComments } from "./chat-comment-pills";

export function ActionButton({
  label,
  onClick,
  active,
  children,
}: {
  /** Constant for the lifetime of the button — see the jitter note. */
  label: string;
  onClick: () => void;
  /** Sticky "on" state (the pin). Hover styling still applies on top. */
  active?: boolean;
  children: React.ReactNode;
}) {
  return (
    <HintItem label={label}>
      <button
        type="button"
        onClick={onClick}
        aria-pressed={active}
        className={cn(
          "flex h-5 w-5 items-center justify-center rounded-md cursor-pointer",
          "hover:bg-[var(--card)] hover:text-[var(--foreground)]",
          active ? "text-[var(--primary)]" : "text-[var(--muted-foreground)]",
        )}
      >
        {children}
      </button>
    </HintItem>
  );
}

/** The copy action with its 1.2 s tick, shared by both bars. */
export function useCopy(text: string): { copied: boolean; onCopy: () => void } {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  // A row can unmount while the "copied" tick is still pending — a history
  // load replaces the projection wholesale, and closing the tab takes the
  // transcript with it. (Window growth does not: `key={row.id}` keeps existing
  // instances alive when rows are prepended.)
  useEffect(() => () => void (timer.current && clearTimeout(timer.current)), []);
  const onCopy = useCallback(() => {
    void copyText(text).then((ok) => {
      // `copyText` returns false rather than throwing when both the native and
      // the web path fail. Swallowing that is indistinguishable from success —
      // the tick simply never appears and the user pastes stale clipboard
      // contents somewhere else before noticing.
      if (!ok) {
        toast.error("Could not copy to the clipboard");
        return;
      }
      setCopied(true);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), 1_200);
    });
  }, [text]);
  return { copied, onCopy };
}

/**
 * Copy, with the comment pill beside it when the message is in the cloud.
 *
 * The two share one surround, as on the Timeline: the pill is how a
 * discussion announces itself, and a lone pill with a gap beside it reads as
 * something missing. Without a cloud anchor this is the plain copy button —
 * no surround around a single control.
 */
export function CommentAndCopy({
  tabId,
  messageId,
  text,
  label,
}: {
  tabId: string;
  messageId: string;
  text: string;
  label: string;
}) {
  const { copied, onCopy } = useCopy(text);
  const anchor = useAnchorHit(tabId, messageId);
  const comments = useCommentBucket(tabId, messageId);
  const commentActions = useCommentActions(tabId);
  const directory = useCommentDirectory(tabId);
  const copy = (
    <ActionButton label={label} onClick={onCopy}>
      <CopyGlyph copied={copied} size="sm" />
    </ActionButton>
  );
  if (!anchor || !commentActions || !directory) return copy;
  return (
    // The surround's OUTER edge is what the eye lines up with the bubble, so
    // the cluster sits flush with the bar's end and the icons inset inside
    // it. Pulling it out to keep the copy icon where it stands alone was
    // tried and read as the pill overhanging the bubble (measured headless:
    // the pill's border landed 4px past the bubble's edge).
    <ActionCluster reveal="snap" pinned={visibleCount(comments) > 0}>
      <CommentButton
        bare
        className="transition-none"
        anchorKind={anchor.anchorKind}
        anchorId={anchor.rowId}
        comments={comments}
        actions={commentActions}
        directory={directory}
      />
      {copy}
    </ActionCluster>
  );
}

export function UserRowActions({
  tabId,
  text,
  canRetry,
  messageId,
  timestamp,
  pinScopeKey,
  toggleAbove,
}: {
  tabId: string;
  /** The cleaned prompt — `row.text`, already stripped of injected context and
   *  the next-steps directive by `derivedUser`. Copying the wire text instead
   *  would hand the user kilobytes of repo context they never wrote. */
  text: string;
  /** Only the thread's last user message can be retried, and only where the
   *  agent can actually rewind. Resolved by the transcript so this stays a
   *  plain boolean prop — see `sessionCanRetry` in `retry-gate.ts`. */
  canRetry: boolean;
  /** `ChatMessage.id` — what a pin addresses. The row id is `u:<messageId>`;
   *  the pin stores the raw id so the header can resolve it to a message
   *  index for `atlas:chat-jump`. */
  messageId: string;
  /** The message's own timestamp — the durable half of a pin's key, since ids
   *  are re-minted on every history load (`resolvePinIndex`). */
  timestamp: string;
  /** Pin scope for this thread, resolved by the transcript — see `pinScope`.
   *  Rows must not read the chat store themselves (house rule 3). */
  pinScopeKey: string;
  /** Is the "Show more" toggle rendered between the bubble and this bar? Then
   *  the gap is already there and this must not add a second one — see the
   *  padding below. Resolved by the row via `clampable`, so the toggle and
   *  this answer can never disagree. */
  toggleAbove: boolean;
}) {
  // The one subscription a row is allowed. It is not the chat store: the pins
  // store is written only when someone clicks a pin, so this never fires on a
  // streaming frame, and the selector returns a boolean.
  const pinned = useChatPinsStore((s) =>
    (s.pins[pinScopeKey] ?? []).some((p) => p.messageId === messageId),
  );
  // The other permitted subscription: the comments store, written only when a
  // comment arrives or the session's cloud identity resolves — never on a
  // streaming frame. A boolean, so the bar re-renders only when it flips.
  const discussed = useRowHasComments(tabId, messageId);

  const onRetry = useCallback(() => void retryLastTurn(tabId), [tabId]);

  // Edit is NOT retry. Retry rewinds the turn off the agent and re-runs it
  // (destructive, native-agent-only). ACP has no verb to forget or rewrite a
  // turn (see `retry-gate.ts`), so editing in place is impossible there; the
  // honest version every agent supports is to load the prompt back into the
  // composer and let it go out as a NEW turn, the old exchange left intact.
  // Sending unchanged is one Enter away, which is what the old blind "Send
  // this prompt again" button did. The composer owns the draft, so this goes
  // through its tab-scoped `atlas:chat-prefill` seam.
  const onEdit = useCallback(() => {
    window.dispatchEvent(new CustomEvent("atlas:chat-prefill", { detail: { text, tabId } }));
  }, [text, tabId]);

  const onPin = useCallback(() => {
    useChatPinsStore.getState().actions.toggle(pinScopeKey, {
      messageId,
      timestamp,
      text,
      at: new Date().toISOString(),
    });
  }, [pinScopeKey, messageId, timestamp, text]);

  return (
    <HintGroup>
      <div
        className={cn(
          // `top-full`, not "under the bubble": the attachment chip sits below
          // the bubble too, and anchoring to the bubble would drop the bar on
          // top of it.
          // `w-max`: the bar is wider than a short bubble, and an absolutely
          // positioned box with `right: 0` and no width is shrink-to-fit
          // against its containing block — WebKit clamped it to the bubble's
          // 95px and let the 109px of controls overflow to the right (measured
          // in the running app: pill 14px past the bubble). Max-content sizes
          // the box to its controls, so `right-0` puts the last one on the
          // bubble's edge and the rest extend left.
          "absolute right-0 top-full z-popover flex w-max items-center gap-0.5",
          // The gap above the icons, as padding rather than a margin so the
          // bar's box still starts exactly at `top-full`. Only the top half
          // draws anything; the bottom 8px is empty and free to overhang the
          // row's `pb-7`.
          //
          // A bubble with a "Show more" toggle already has that gap: the toggle
          // is in flow between the bubble and this bar, so `top-full` is below
          // IT, and a top pad here would stack on top of the toggle's own
          // height — the icons visibly sat further from a clamped bubble than
          // from a short one. Longhands in both branches, never `py-2` plus a
          // `pt-0` override: shorthand-vs-longhand precedence is decided by
          // stylesheet order, which is not something to bet spacing on.
          toggleAbove ? "pt-0 pb-2" : "pt-2 pb-2",
          // Hidden until the row is hovered, and it SNAPS — no transition, no
          // fade, nothing to interpolate. `visibility` rather than `opacity`
          // because an `opacity-0` bar is still hit-testable: it could be
          // clicked while invisible. `focus-within` is not decoration either —
          // without it, keyboard users would tab into controls they cannot see.
          // A discussed prompt keeps its bar: the pill is how the discussion
          // announces itself, and it must not vanish with the pointer.
          discussed ? "visible" : "invisible group-hover:visible focus-within:visible",
        )}
      >
        {canRetry && (
          <ActionButton label="Retry (replaces this response)" onClick={onRetry}>
            <RefreshCw size={12} />
          </ActionButton>
        )}
        <ActionButton label="Pin message" onClick={onPin} active={pinned}>
          <Pin size={12} fill={pinned ? "currentColor" : "none"} />
        </ActionButton>
        {/* A turn-out arrow, not a pencil. Nothing is edited in place — the
            prompt goes back to the composer and leaves as a NEW turn, which is
            the shape this glyph has carried here since it was a plain resend. */}
        <ActionButton label="Edit and send as new message" onClick={onEdit}>
          <CornerUpRight size={12} />
        </ActionButton>
        <CommentAndCopy tabId={tabId} messageId={messageId} text={text} label="Copy message" />
      </div>
    </HintGroup>
  );
}
