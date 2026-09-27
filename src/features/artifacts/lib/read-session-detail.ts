/**
 * Where an open Session's timeline is read from.
 *
 * The board row carries two addresses: `projectPath` (a checkout on this
 * machine, when there is one) and `remoteProjectId` (the server's Project).
 * The local store is the first choice because it is instant and offline — but
 * a `projectPath` is set for every Session of a Project this machine has bound,
 * **including a teammate's Sessions that only ever existed on the server**.
 * Reading those locally answers "no such Session", which the panel used to
 * render as "This session no longer exists." while the web showed it fine.
 *
 * So the rule is local first, then the server, and only then absent.
 */

import { invoke } from "@tauri-apps/api/core";

import type { OpenSession } from "../stores/artifacts-store";
import type { SessionDetail } from "../types";

export interface DetailSources {
  /** This machine's store for the Project at `projectPath`; `null` when the
   *  Session is not in it. */
  local: (projectPath: string, sessionId: string) => Promise<SessionDetail | null>;
  /** The server's copy, paged and folded into the local shape by Rust. */
  remote: (remoteProjectId: string, sessionId: string) => Promise<SessionDetail>;
}

const tauriSources: DetailSources = {
  local: (projectPath, sessionId) =>
    invoke<SessionDetail | null>("artifacts_session", { projectPath, sessionId }),
  remote: (projectId, sessionId) =>
    invoke<SessionDetail>("artifacts_cloud_session", { projectId, sessionId }),
};

/**
 * Read the Session `open` points at, or `null` when neither side has it.
 *
 * A remote read is attempted only when the local one came back empty and the
 * row is shared — a Session that is local-only has nowhere else to be, and
 * asking the server about it would be a 404 dressed as a delay.
 */
export async function readSessionDetail(
  open: Pick<OpenSession, "sessionId" | "projectPath" | "remoteProjectId">,
  sources: DetailSources = tauriSources,
): Promise<SessionDetail | null> {
  const remoteProjectId = open.remoteProjectId ?? null;
  if (open.projectPath) {
    const local = await sources.local(open.projectPath, open.sessionId);
    if (local) return local;
  }
  if (!remoteProjectId) return null;
  return sources.remote(remoteProjectId, open.sessionId);
}
