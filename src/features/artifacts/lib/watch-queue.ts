/**
 * The realtime subscription, one call at a time.
 *
 * `artifacts_cloud_follow` / `_unfollow` are issued from a React effect: follow
 * on mount, unfollow on cleanup. Tauri runs each invoke on its own task, so
 * two issued back to back can land in either order — and under StrictMode a
 * mount is `follow, unfollow, follow`. Landing as `follow, follow, unfollow`
 * would leave the refcount one short of the watchers actually mounted, and
 * the socket would close under the surviving one.
 *
 * So every call goes through one promise chain, in the order it was asked.
 * A failed call is dropped, not propagated: a chain that stayed rejected
 * would refuse every follow after the first failure.
 */

import { invoke } from "@tauri-apps/api/core";

let chain: Promise<void> = Promise.resolve();

function enqueue(call: () => Promise<unknown>): Promise<void> {
  chain = chain.then(call).then(
    () => undefined,
    () => undefined,
  );
  return chain;
}

export function queueFollow(projectId: string, sessionId: string): Promise<void> {
  return enqueue(() => invoke("artifacts_cloud_follow", { projectId, sessionId }));
}

export function queueUnfollow(projectId: string, sessionId: string): Promise<void> {
  return enqueue(() => invoke("artifacts_cloud_unfollow", { projectId, sessionId }));
}
