/**
 * The wire of a **UI action** (CONTEXT.md; ADR-0012): one thing Atlas Agent
 * asked the window to do, and the window's answer.
 *
 * Rust emits the request as `atlas:ui-action` and forwards the reply to the
 * model verbatim, so everything a tool means — and every word of an error the
 * model reads — is decided on this side.
 */

export interface UiActionRequest {
  requestId: string;
  /** The calling session's ACP session id. */
  sessionId: string;
  agent: string;
  /** The calling session's working directory. */
  cwd: string;
  /** The tool the model called, e.g. `ui_state`. */
  tool: string;
  /** The tool's arguments exactly as the model sent them. */
  args: Record<string, unknown>;
}

export type UiActionReply = { ok: true; result: unknown } | { ok: false; error: string };

export const ok = (result: unknown): UiActionReply => ({ ok: true, result });
export const fail = (error: string): UiActionReply => ({ ok: false, error });
