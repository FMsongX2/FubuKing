// Modified by FubuMem from upstream Atlas (Apache-2.0).
export const TAB_TYPES = [
  "chat",
  "canvas",
  "browser",
  "tasks",
  "editor",
  "knowledge",
  "knowledge-graph",
  "memory",
  "terminal",
  "diff",
  "settings",
  "log",
  "media",
  "svg",
  "pdf",
  "unsupported",
  "usage",
  "quota",
  "artifacts",
  "comms-draft",
  "spaces",
] as const;

export type TabType = (typeof TAB_TYPES)[number];

/**
 * Tab types that work with NO project open — org-scoped surfaces, not
 * project ones. The projectless centre shell renders only these; the rest
 * of a project's tabs stay in the store untouched and reappear when a
 * project opens.
 */
export const PROJECTLESS_TYPES: ReadonlySet<TabType> = new Set<TabType>([
  "settings",
  "comms-draft",
  "spaces",
  "usage",
  "quota",
]);

/**
 * Tab types whose CONTENT belongs to one Organisation — their ids and data
 * embed conversation/draft ids that exist in exactly one org.
 *
 * Two consequences, both load-bearing:
 *  - they are closed on every org change (`switchOrg`, and the boot
 *    reconciliation branch in the comms store);
 *  - they are NEVER written to the per-project editor state. That file is
 *    keyed by PROJECT PATH, and the same path is commonly open in several
 *    orgs — a persisted Space tab came back on the incoming org's mount
 *    pointing at the outgoing org's conversation ("This Space is no longer
 *    available"), which is what closing alone could not fix.
 *
 * Settings is deliberately absent: it is projectless but org-agnostic.
 */
export const ORG_SCOPED_TYPES: ReadonlySet<TabType> = new Set<TabType>([
  "comms-draft",
  "spaces",
  "usage",
]);

/**
 * Tab types that were renamed. Persisted layout / editor state written by an
 * older build still carries the old name; `migrateTabType` maps it forward so
 * the tab comes back instead of being dropped as unknown.
 */
export const LEGACY_TAB_TYPES: Record<string, TabType> = { "mission-control": "usage" };

/** The current name for a stored tab type: the mapped type for a legacy name,
 *  the type itself when it is still valid, `null` when it is neither. */
export function migrateTabType(type: string): TabType | null {
  const legacy = LEGACY_TAB_TYPES[type];
  if (legacy) return legacy;
  return (TAB_TYPES as readonly string[]).includes(type) ? (type as TabType) : null;
}
