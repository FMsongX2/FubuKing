/**
 * Who is in the active Organisation — one hook, for every surface that has an
 * id and needs a face.
 *
 * A server-side id is all the wire ever carries. A board row's `authorId`, a
 * comment's `authorId`, a `<@mention>` — every one of them is an opaque key
 * that has to be turned into a name and a photo, and every one of them was
 * solving it separately. The Timeline's byline read the *org* roster keyed by
 * `userId`; the comment thread read the *chat* roster keyed by `id`, which is a
 * different list under a different key, so it matched nothing and rendered raw
 * ids. That is the bug this exists to make unrepeatable.
 *
 * # What it owns
 *
 * Fetching (stale-while-revalidate through `members-store`, which has its own
 * freshness window and in-flight guard, so calling this from ten components is
 * one request), caching, and the lookup key. Callers get a map and nothing to
 * decide.
 *
 * # The key is `userId`, never `id`
 *
 * `OrgMember.id` is the **membership** id — it differs for the same person in
 * every Organisation they belong to. `OrgMember.userId` is the human, and it is
 * what the server stamps on a Session and a comment. A directory keyed by the
 * membership id looks right, compiles, and never matches anything.
 */

import { useEffect, useMemo } from "react";

import type { OrgMember } from "@/features/auth/lib/auth-api";
import { useAuthStore } from "@/features/auth/stores/auth-store";

import { useMembersStore } from "../stores/members-store";
import { useOrgStore } from "../stores/org-store";

export interface OrgDirectory {
  /** Members of the active Organisation, by the id that identifies the human. */
  byId: Map<string, OrgMember>;
  /** The signed-in account, so a surface can say "You" rather than a name. */
  currentUserId: string | null;
}

export const EMPTY_ORG_DIRECTORY: OrgDirectory = { byId: new Map(), currentUserId: null };

/**
 * One shared empty roster.
 *
 * A fresh `[]` each render would rebuild the map and hand every consumer a new
 * directory identity — which defeats `memo` on the five hundred rows of the
 * Timeline nav that take it as a prop.
 */
const EMPTY_MEMBERS: OrgMember[] = [];

export function useOrgDirectory(): OrgDirectory {
  const authSnapshot = useAuthStore.use.snapshot();
  const currentUserId =
    authSnapshot.status === "signed-in" ? (authSnapshot.user?.id ?? null) : null;

  const organisations = useOrgStore.use.organisations();
  const activeOrganisationId = useOrgStore.use.activeOrganisationId();
  // The **server** org id. A local-only Organisation has none and no roster to
  // fetch — everything in it belongs to this account anyway.
  const remoteOrgId = organisations.find((o) => o.id === activeOrganisationId)?.remoteId ?? null;

  const byOrg = useMembersStore.use.byOrg();
  const { load } = useMembersStore.use.actions();
  useEffect(() => {
    if (remoteOrgId) void load(remoteOrgId);
  }, [remoteOrgId, load]);

  const members = remoteOrgId ? (byOrg[remoteOrgId]?.members ?? EMPTY_MEMBERS) : EMPTY_MEMBERS;
  return useMemo(
    () => ({ byId: new Map(members.map((m) => [m.userId, m])), currentUserId }),
    [members, currentUserId],
  );
}
