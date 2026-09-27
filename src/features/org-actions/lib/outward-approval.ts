/**
 * An **outward action**'s approval card (CONTEXT.md; ADR-0014): a call on the
 * organisation tool server that reaches another person in the user's name —
 * a reply on a comment thread, a message — stopped before anything leaves the
 * device.
 *
 * Only the organisation server's outward tools ever ask: every other tool on
 * it is auto-approved, so an organisation call that reaches the approval card
 * is an outward action by construction. And only the native agent's cards can
 * match: `atlas_org` is offered only to a connection that carries
 * organisation access (the in-process native connection, never an ACP one),
 * so an ACP agent's card never names one of its tools. The native seam titles the card with
 * the act ("Reply on Ada Lovelace's comment") and gives the call two text
 * blocks, the recipient and then the full body
 * (`crates/atlas-native-agent/src/engine/tool_approvals.rs`); the wire keeps
 * the tool's own name beside the title (`permission_tool_call`,
 * `crates/atlas-agent-delta/src/project.rs`).
 */

import type { ToolCallRef } from "@/types/acp";
import { orgToolOf } from "./org-tool-rows";

export interface OutwardApproval {
  /** The act and whom it reaches: "Reply on Ada Lovelace's comment". */
  title: string;
  /** Who and where it reaches, in full. */
  recipient: string;
  /** The exact words that will be posted, never shortened. */
  body: string;
}

/** The card's text, or `null` for any approval that is not an outward
 *  action (or one the host could not describe, which keeps the plain card). */
export function outwardApprovalOf(toolCall: ToolCallRef): OutwardApproval | null {
  const toolName = typeof toolCall.toolName === "string" ? toolCall.toolName : "";
  if (orgToolOf(toolName) === null) return null;
  const content = Array.isArray(toolCall.content)
    ? toolCall.content.filter((c): c is string => typeof c === "string")
    : [];
  const title = typeof toolCall.title === "string" ? toolCall.title.trim() : "";
  if (content.length < 2 || !title) return null;
  const [recipient, ...body] = content;
  return { title, recipient, body: body.join("\n") };
}

/** An outward action's card while it is still being prepared. */
export interface OutwardPreparing {
  /** "Preparing the approval…" — the native seam's title for it. */
  title: string;
  /** What is being looked up, and that Decline works already. */
  note: string;
}

/**
 * The card that is up while the host describes an outward action, or `null`.
 * The native seam raises it at once with **only Decline** and replaces it with
 * the described card when the recipient and body are known
 * (`tool_approvals.rs`, "Preparing, then the card"): an organisation tool's
 * card that offers nothing to allow is that card. Never shown with the call's
 * arguments — there is nothing to approve yet.
 */
export function outwardPreparingOf(
  toolCall: ToolCallRef,
  options: ReadonlyArray<{ kind: string }>,
): OutwardPreparing | null {
  const toolName = typeof toolCall.toolName === "string" ? toolCall.toolName : "";
  if (orgToolOf(toolName) === null) return null;
  if (options.some((o) => o.kind === "allow_once" || o.kind === "allow_always")) return null;
  const content = Array.isArray(toolCall.content)
    ? toolCall.content.filter((c): c is string => typeof c === "string")
    : [];
  const title = typeof toolCall.title === "string" && toolCall.title.trim() ? toolCall.title : "";
  return {
    title: title || "Preparing the approval…",
    note: content[0] ?? "Looking up who this reaches and the exact words it will post.",
  };
}

/** Whether `toolCall` is an outward action's card — any organisation tool's,
 *  since only the outward ones ever ask. */
export function isOutwardCall(toolCall: ToolCallRef): boolean {
  const toolName = typeof toolCall.toolName === "string" ? toolCall.toolName : "";
  return orgToolOf(toolName) !== null;
}

/**
 * Whether a keystroke (Enter, or the option's digit) may pick an option of
 * `kind` on this card. An outward action posts in the user's name and cannot
 * be taken back, and the card's keys listen on the whole window — an Enter
 * meant for the composer as the card appeared would post. So its Allow is a
 * click, never a key; Decline and Esc still work from the keyboard.
 */
export function keyMayPick(toolCall: ToolCallRef, kind: string): boolean {
  const allow = kind === "allow_once" || kind === "allow_always";
  return !(allow && isOutwardCall(toolCall));
}
