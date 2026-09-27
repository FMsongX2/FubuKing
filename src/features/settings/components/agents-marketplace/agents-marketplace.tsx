// Settings → Agents. The ACP Registry marketplace (modeled on Zed's): every
// agent published to the official ACP registry, searchable, with
// All/Installed/Not-Installed pills and per-card Install/Remove.
//
// This is the ONLY way an ACP agent comes to exist (ADR-0002): Atlas
// ships none, so no card can say "built-in" and there is nothing to switch
// off. An agent is installed, merely detected on the user's PATH, or an offer.
//
// Install state machine mirrors skills-marketplace.tsx: in-flight ids live at
// MODULE scope behind useSyncExternalStore so a mid-install unmount (switching
// settings sections) doesn't lose the spinner; binary downloads stream
// progress via `atlas:registry-install:progress`.

import { memo, useCallback, useEffect, useMemo, useState, useSyncExternalStore } from "react";
import { listen } from "@tauri-apps/api/event";
import { openUrl } from "@tauri-apps/plugin-opener";
import { ArrowUpCircle, Check, Download, Globe, Loader2, RefreshCw, Search, X } from "lucide-react";
import { GithubIcon } from "@/components/github-icon";
import { toast } from "sonner";

import { cn } from "@/lib/utils";
import { HintGroup, HintItem } from "@/ui/hint-group";
import { Hint } from "@/ui/tooltip";
import { AgentMonogram, ExternalAgentIcon } from "@/components/agent-icons";
import {
  acpRegistry,
  type AcpRegistryEntry,
  type RegistryInstallProgress,
} from "@/features/agents/lib/agent-registry-api";
import {
  hydrateAgentRegistry,
  refreshAgentRegistry,
  useAgentRegistryStore,
  type AgentUpdatePhase,
} from "@/features/agents/stores/agent-registry-store";
import { updateAgent, updatePhaseLabel } from "@/features/agents/lib/agent-update";
import type { AgentCatalogEntry } from "@/types/agent-catalog";
import { useRemoveAgentConfirmStore } from "@/features/agents/lib/remove-agent-confirm";
import { downloadTrend, fmtDownloads } from "@/features/agents/lib/download-trends";
import { TrendSparkline } from "@/components/trend-sparkline";

/** Which action a card offers. Pure and exported so the precedence is tested
 *  rather than re-read out of JSX.
 *
 *  "detected" is the system-first addition: the agent is on the user's PATH and
 *  spawns from there, but Atlas never installed it — so offering "Remove" would
 *  be a lie, and offering "Install" alone would hide that it already works. */
export function cardState(
  entry: AcpRegistryEntry,
  catalog: AgentCatalogEntry | undefined,
): "installed" | "detected" | "install" {
  if (entry.installed) return "installed";
  if (catalog?.source === "detected") return "detected";
  return "install";
}

/** Which install a card's action performs.
 *
 *  A detection is ACCEPTED — the installed-agents map gets a `custom` entry pointing
 *  at the binary already on the user's machine, so Atlas runs their copy. Every
 *  other card downloads Atlas's own. Both write the same map; only the entry
 *  differs, and that difference is the whole point of a detection. */
export function installKind(state: ReturnType<typeof cardState>): "detected" | "registry" {
  return state === "detected" ? "detected" : "registry";
}

/** A card for an agent the registry listing doesn't cover — one found on the
 *  user's PATH, or one they installed that was never published. Deduped by id
 *  against the real listing by [`marketplaceCards`]. */
function syntheticCard(entry: AgentCatalogEntry): AcpRegistryEntry {
  return {
    id: entry.id,
    name: entry.name,
    version: entry.version ?? "",
    description: entry.description,
    repository: entry.repository,
    website: entry.website,
    iconDataUrl: entry.iconDataUrl,
    installed: entry.installed,
    // Not a platform claim from a manifest: this agent is on THIS machine, so
    // the only way it is unsupported here is the catalog saying its install
    // resolves to nothing runnable.
    platformSupported: entry.source !== "unavailable",
    distributionKind: entry.distributionKind,
    unverified: entry.unverified,
    unsupportedReason: null,
    installedVersion: null,
    updateAvailable: false,
  };
}

/** Every card the marketplace shows: the registry's own listing, plus one
 *  synthesized card per agent Atlas knows about that the listing has never
 *  heard of.
 *
 *  That second group is both halves of "off the registry": an agent merely
 *  found on the user's PATH, and one they have already installed from there.
 *  Dropping the installed half is what once made a hand-installed agent
 *  disappear from Settings the moment it was accepted — taking its only
 *  Remove button with it.
 *
 *  The native agent is never a card: it is in-process, so there is nothing to
 *  install and nothing to remove. */
export function marketplaceCards(
  listed: AcpRegistryEntry[],
  catalogById: Record<string, AgentCatalogEntry>,
): AcpRegistryEntry[] {
  const listedIds = new Set(listed.map((e) => e.id));
  const offRegistry = Object.values(catalogById)
    .filter((e) => e.kind !== "native" && !listedIds.has(e.id))
    // catalogById is keyed by BOTH id and agentType, so one entry can appear
    // twice — dedupe before synthesizing cards.
    .filter((e, i, all) => all.findIndex((o) => o.id === e.id) === i)
    .map(syntheticCard);
  return [...listed, ...offRegistry];
}

// ── Module-scope install tracking (survives unmount) ─────────────────────────

const installingIds = new Set<string>();
let installVersion = 0;
const installSubs = new Set<() => void>();
function notifyInstalling() {
  installVersion++;
  installSubs.forEach((f) => f());
}
function subscribeInstalling(cb: () => void) {
  installSubs.add(cb);
  return () => installSubs.delete(cb);
}

/** Latest download progress per agent id (binary distributions only). */
const progressById = new Map<string, RegistryInstallProgress>();

type Filter = "all" | "installed" | "not-installed";

/** How long a listing stays good enough to show without going back out to the
 *  network. Matches the backend's own refresh throttle, so a revalidate inside
 *  this window would be a no-op round trip anyway. */
const STALE_AFTER_MS = 60 * 60 * 1000;

export function AgentsMarketplace() {
  const [query, setQuery] = useState("");
  const [filter, setFilter] = useState<Filter>("all");
  useSyncExternalStore(subscribeInstalling, () => installVersion);

  // The listing lives in the store, prefetched at startup and kept current by
  // `atlas:agent-catalog:changed` — this panel reads it rather than fetching on
  // mount. Opening Settings → Agents therefore paints what is already known,
  // instead of racing boot's own refresh and painting "Registry unavailable"
  // until the user pressed Refresh by hand.
  const loaded = useAgentRegistryStore((s) => s.registryLoaded);
  const refreshing = useAgentRegistryStore((s) => s.registryRefreshing);
  const error = useAgentRegistryStore((s) => s.registryError);
  const refreshedAt = useAgentRegistryStore((s) => s.registryRefreshedAt);

  const refresh = useCallback(async () => {
    await refreshAgentRegistry();
    const { registryError } = useAgentRegistryStore.getState();
    // Only shout when there is nothing to fall back on. With cached entries on
    // screen the inline "showing cached data" note says it without a toast.
    if (registryError && useAgentRegistryStore.getState().registryEntries.length === 0) {
      toast.error(`Couldn't load the ACP registry: ${registryError}`);
    }
  }, []);

  // Stale-while-revalidate: paint the cache now, confirm it in the background,
  // and only reach for the network when what we hold is actually old.
  //
  // The local re-read is unconditional: it is what re-measures each installed
  // agent's copy on disk against the registry, and an agent that installed
  // since the last listing (a first chat, a version bump) emits nothing.
  useEffect(() => {
    const { registryRefreshedAt } = useAgentRegistryStore.getState();
    void hydrateAgentRegistry();
    const age = registryRefreshedAt ? Date.now() - Date.parse(registryRefreshedAt) : Infinity;
    if (!Number.isFinite(age) || age > STALE_AFTER_MS) void refresh();
  }, [refresh]);

  // Nothing cached and the last attempt failed — retry once on its own, so the
  // first thing the user has to do on this screen isn't press a button.
  const [autoRetried, setAutoRetried] = useState(false);
  const entryCount = useAgentRegistryStore((s) => s.registryEntries.length);
  useEffect(() => {
    if (autoRetried || refreshing || !error || entryCount > 0) return;
    setAutoRetried(true);
    const timer = setTimeout(() => void refresh(), 1500);
    return () => clearTimeout(timer);
  }, [autoRetried, refreshing, error, entryCount, refresh]);

  // Binary-download progress → re-render the affected card.
  useEffect(() => {
    const unlisten = listen<RegistryInstallProgress>("atlas:registry-install:progress", (event) => {
      progressById.set(event.payload.agentId, event.payload);
      notifyInstalling();
    });
    return () => {
      void unlisten.then((f) => f());
    };
  }, []);

  const install = useCallback(async (entry: AcpRegistryEntry, kind: "detected" | "registry") => {
    if (installingIds.has(entry.id)) return;
    installingIds.add(entry.id);
    notifyInstalling();
    try {
      if (kind === "detected") await acpRegistry.installDetected(entry.id);
      else await acpRegistry.install(entry.id);
      toast.success(`${entry.name} installed`);
    } catch (e) {
      toast.error(`Couldn't install ${entry.name}: ${String(e)}`);
    } finally {
      installingIds.delete(entry.id);
      progressById.delete(entry.id);
      notifyInstalling();
    }
    // One re-read now that the installed map moved: it refreshes the cards'
    // Install/Remove state and every other agent surface at once.
    await hydrateAgentRegistry();
  }, []);

  const update = useCallback((entry: AcpRegistryEntry) => {
    void updateAgent(entry.id, entry.name, entry.version);
  }, []);

  const uninstall = useCallback(async (entry: AcpRegistryEntry) => {
    try {
      await acpRegistry.uninstall(entry.id);
      toast.success(`${entry.name} removed`);
    } catch (e) {
      toast.error(`Couldn't remove ${entry.name}: ${String(e)}`);
    }
    // One re-read now that the installed map moved: it refreshes the cards'
    // Install/Remove state and every other agent surface at once.
    await hydrateAgentRegistry();
  }, []);

  // Catalog-backed annotations: which of these the user already has on their
  // system. Subscribes to the primitive signature (Record selectors
  // infinite-loop under useShallow — the store's documented trap).
  const signature = useAgentRegistryStore((s) => s.signature);
  const updatePhases = useAgentRegistryStore((s) => s.updatePhases);
  const { catalogById, registryEntries } = useAgentRegistryStore.getState();

  const entries = useMemo(() => {
    const all = marketplaceCards(registryEntries, catalogById);
    const q = query.trim().toLowerCase();
    return all.filter((e) => {
      const state = cardState(e, catalogById[e.id]);
      // "Installed" means "already available to you", which a detected agent
      // is — it just wasn't Atlas that put it there.
      const have = e.installed || state === "detected";
      if (filter === "installed" && !have) return false;
      if (filter === "not-installed" && have) return false;
      if (!q) return true;
      return (
        e.name.toLowerCase().includes(q) ||
        e.id.toLowerCase().includes(q) ||
        (e.description ?? "").toLowerCase().includes(q)
      );
    });
    // `signature` is the primitive that changes when registryEntries/catalog do;
    // the arrays themselves are read via getState() and are not stable refs.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [signature, query, filter]);

  /** Cards for agents Atlas found rather than installed, rendered under their
   *  own heading so "you already have this" reads as a fact, not an offer. */
  const detectedIds = useMemo(
    () =>
      new Set(
        entries.filter((e) => cardState(e, catalogById[e.id]) === "detected").map((e) => e.id),
      ),
    [entries, catalogById],
  );

  return (
    <div className="h-full flex flex-col">
      {/* Header — a single row: search + filter pills left, refresh right. */}
      <div className="shrink-0 px-4 py-2 border-b border-[var(--border)]">
        <div className="flex items-center gap-2">
          <div className="relative flex-1 max-w-[320px]">
            <Search
              size={12}
              className="absolute left-2.5 top-1/2 -translate-y-1/2 text-[var(--muted-foreground)]"
            />
            <input
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              placeholder="Search registry"
              className="w-full h-7 pl-7 pr-7 rounded-md bg-[var(--card)] border border-[var(--border)] text-sm text-[var(--foreground)] placeholder:text-[var(--muted-foreground)] outline-none focus:border-border-strong"
            />
            {query && (
              <Hint label="Clear search">
                <button
                  onClick={() => setQuery("")}
                  className="absolute right-2 top-1/2 -translate-y-1/2 text-[var(--muted-foreground)] hover:text-[var(--foreground)] cursor-pointer"
                >
                  <X size={11} />
                </button>
              </Hint>
            )}
          </div>
          <div className="inline-flex items-center gap-0.5 rounded-full border border-[var(--border)] bg-[var(--card,var(--card))] p-0.5">
            {(
              [
                ["all", "All"],
                ["installed", "Installed"],
                ["not-installed", "Not Installed"],
              ] as const
            ).map(([id, label]) => (
              <button
                key={id}
                onClick={() => setFilter(id)}
                className={cn(
                  "h-[22px] px-2.5 rounded-full text-xs font-medium transition-colors cursor-pointer",
                  filter === id
                    ? "bg-[var(--atlas-element-selected,var(--atlas-element-hover))] text-[var(--foreground)]"
                    : "text-[var(--muted-foreground)] hover:text-[var(--secondary-foreground)]",
                )}
              >
                {label}
              </button>
            ))}
          </div>
          <span className="flex-1" />
          {/* Background refresh is visible but not in the way: cards stay put and
              readable underneath while the label says what is happening. */}
          <button
            onClick={() => void refresh()}
            disabled={refreshing}
            className="flex items-center gap-1.5 h-6 px-2 rounded-md text-xs text-[var(--secondary-foreground)] hover:bg-[var(--atlas-element-hover)] hover:text-[var(--foreground)] transition-colors cursor-pointer disabled:cursor-default disabled:hover:bg-transparent"
            title={
              refreshedAt
                ? `Last updated ${new Date(refreshedAt).toLocaleString()}`
                : "Refresh the registry"
            }
          >
            <RefreshCw size={11} className={cn(refreshing && "animate-spin")} />
            {refreshing ? "Refreshing…" : "Refresh"}
          </button>
        </div>
        {error && registryEntries.length > 0 && (
          <p className="text-2xs text-[var(--muted-foreground)]">
            Last refresh failed ({error}) — showing cached data
            {refreshedAt ? ` from ${new Date(refreshedAt).toLocaleString()}` : ""}.
          </p>
        )}
      </div>

      {/* Card list. */}
      {/* hide-scrollbar: the grid is two/three columns of fixed-width cards, so
          a gutter-reserving scrollbar costs real card width. Matches the skills
          Discover table next door. */}
      <div className="hide-scrollbar min-h-0 flex-1 overflow-y-auto px-4 py-3">
        {registryEntries.length === 0 && (!loaded || refreshing) ? (
          // Nothing cached yet AND still working — the only case that earns a
          // blocking spinner. With a cache in hand we always paint the cache.
          <div className="h-full flex flex-col items-center justify-center gap-2">
            <Loader2 size={18} className="animate-spin text-[var(--muted-foreground)]" />
            <p className="text-xs text-[var(--muted-foreground)]">Loading the ACP registry…</p>
          </div>
        ) : entries.length === 0 ? (
          <div className="h-full flex flex-col items-center justify-center gap-2 text-center">
            <p className="text-sm text-[var(--muted-foreground)]">
              {registryEntries.length > 0
                ? "No agents match."
                : error
                  ? "Couldn't reach the ACP registry."
                  : "The ACP registry is empty."}
            </p>
            {registryEntries.length === 0 && error && (
              <>
                <p className="max-w-[380px] text-xs text-[var(--muted-foreground)]">{error}</p>
                <button
                  onClick={() => void refresh()}
                  disabled={refreshing}
                  className="flex items-center gap-1.5 h-6 px-2.5 rounded-md text-xs font-medium text-[var(--foreground)] border border-[var(--border)] bg-[var(--card,var(--background))] hover:bg-[var(--atlas-element-hover)] transition-colors cursor-pointer disabled:cursor-default"
                >
                  <RefreshCw size={10} className={cn(refreshing && "animate-spin")} />
                  {refreshing ? "Retrying…" : "Try again"}
                </button>
              </>
            )}
          </div>
        ) : (
          <>
            {(["detected", "registry"] as const).map((section) => {
              const inSection = entries.filter((e) =>
                section === "detected" ? detectedIds.has(e.id) : !detectedIds.has(e.id),
              );
              if (inSection.length === 0) return null;
              return (
                <div
                  key={section}
                  className={cn(section === "registry" && detectedIds.size > 0 && "mt-4")}
                >
                  {detectedIds.size > 0 && (
                    <h3 className="mb-1.5 text-xs font-medium uppercase tracking-wide text-[var(--muted-foreground)]">
                      {section === "detected" ? "Detected on your system" : "Registry"}
                    </h3>
                  )}
                  <div className="grid grid-cols-1 lg:grid-cols-2 2xl:grid-cols-3 gap-2.5">
                    {inSection.map((entry) => (
                      <AgentCard
                        key={entry.id}
                        entry={entry}
                        catalog={catalogById[entry.id]}
                        installing={installingIds.has(entry.id)}
                        updatePhase={updatePhases[entry.id] ?? null}
                        progress={progressById.get(entry.id) ?? null}
                        onInstall={install}
                        onUpdate={update}
                        onUninstall={uninstall}
                      />
                    ))}
                  </div>
                </div>
              );
            })}
          </>
        )}
      </div>
    </div>
  );
}

// memo: every install-progress tick bumps the marketplace's version snapshot
// and re-renders the grid — without this, all ~38 cards (icon <img> + trend
// sparkline each) re-rendered per tick instead of just the installing one.
// Handlers take the entry as an argument so the parent can pass its stable
// useCallbacks instead of per-render arrows.
const AgentCard = memo(function AgentCard({
  entry,
  catalog,
  installing,
  updatePhase,
  progress,
  onInstall,
  onUpdate,
  onUninstall,
}: {
  entry: AcpRegistryEntry;
  catalog: AgentCatalogEntry | undefined;
  installing: boolean;
  updatePhase: AgentUpdatePhase | null;
  progress: RegistryInstallProgress | null;
  onInstall: (entry: AcpRegistryEntry, kind: "detected" | "registry") => void;
  onUpdate: (entry: AcpRegistryEntry) => void;
  onUninstall: (entry: AcpRegistryEntry) => void;
}) {
  const pct =
    installing && progress?.total
      ? Math.min(100, (progress.received / progress.total) * 100)
      : null;
  const trend = useMemo(
    () => downloadTrend(entry.id, entry.installed),
    [entry.id, entry.installed],
  );
  return (
    <div className="rounded-lg border border-[var(--border)] bg-[var(--card)] px-3.5 py-3 flex flex-col gap-1.5">
      <div className="flex items-center gap-2.5">
        {/* Explicit color, not inherited: registry icons are monochrome
            `currentColor` art, so the tile is what decides whether they are
            legible. See ExternalAgentIcon. */}
        <span className="flex items-center justify-center size-7 rounded-md border border-[var(--border)] bg-[var(--card,var(--background))] text-[var(--foreground)] shrink-0">
          {entry.iconDataUrl ? (
            <ExternalAgentIcon dataUrl={entry.iconDataUrl} size={16} />
          ) : (
            <AgentMonogram label={entry.name} size={16} />
          )}
        </span>
        <div className="min-w-0 flex-1">
          <div className="flex items-baseline gap-1.5">
            <span className="text-base font-semibold text-[var(--foreground)] truncate">
              {entry.name}
            </span>
            <span className="text-2xs text-[var(--muted-foreground)] tabular-nums">
              v{entry.version}
            </span>
            {entry.updateAvailable && entry.installedVersion && (
              <span
                className="text-2xs text-warning tabular-nums"
                title={`v${entry.installedVersion} is installed; the registry lists v${entry.version}.`}
              >
                installed v{entry.installedVersion}
              </span>
            )}
            {entry.unverified && entry.distributionKind === "binary" && (
              <span
                className="text-3xs px-1 rounded bg-[var(--atlas-element-hover)] text-[var(--muted-foreground)]"
                title="This agent's binary download publishes no checksum."
              >
                unverified
              </span>
            )}
          </div>
          {!entry.platformSupported && (
            <p className="text-2xs text-warning">
              Not supported on this platform
              {entry.unsupportedReason ? ` — ${entry.unsupportedReason}` : ""}
            </p>
          )}
        </div>
        <CardAction
          entry={entry}
          catalog={catalog}
          installing={installing}
          updatePhase={updatePhase}
          pct={pct}
          onInstall={onInstall}
          onUpdate={onUpdate}
          onUninstall={onUninstall}
        />
      </div>
      {entry.description && (
        <p className="text-xs leading-snug text-[var(--secondary-foreground)] line-clamp-2">
          {entry.description}
        </p>
      )}
      <div className="flex items-center gap-2 text-2xs text-[var(--muted-foreground)]">
        <span className="font-mono">ID: {entry.id}</span>
        {entry.distributionKind && <span className="font-mono">[{entry.distributionKind}]</span>}
        <span className="flex-1" />
        {/* 6-month download trend — seeded mock series today, PostHog-backed
            once the `acp_agent_installed` events accrue. Count wears a text
            token; only the sparkline mark carries the trend color. */}
        <span className="tabular-nums">{fmtDownloads(trend.total)}</span>
        <TrendSparkline
          points={trend.points}
          width={64}
          height={20}
          label={`≈${trend.total.toLocaleString()} downloads in the last 6 months`}
        />
        <HintGroup>
          {entry.repository && (
            <HintItem label="Open repository">
              <button
                onClick={() => void openUrl(entry.repository!)}
                className="hover:text-[var(--foreground)] transition-colors cursor-pointer"
              >
                <GithubIcon size={11} />
              </button>
            </HintItem>
          )}
          {entry.website && (
            <HintItem label="Open website">
              <button
                onClick={() => void openUrl(entry.website!)}
                className="hover:text-[var(--foreground)] transition-colors cursor-pointer"
              >
                <Globe size={11} />
              </button>
            </HintItem>
          )}
        </HintGroup>
      </div>
    </div>
  );
});

function CardAction({
  entry,
  catalog,
  installing,
  updatePhase,
  pct,
  onInstall,
  onUpdate,
  onUninstall,
}: {
  entry: AcpRegistryEntry;
  catalog: AgentCatalogEntry | undefined;
  installing: boolean;
  updatePhase: AgentUpdatePhase | null;
  pct: number | null;
  onInstall: (entry: AcpRegistryEntry, kind: "detected" | "registry") => void;
  onUpdate: (entry: AcpRegistryEntry) => void;
  onUninstall: (entry: AcpRegistryEntry) => void;
}) {
  const state = cardState(entry, catalog);
  const kind = installKind(state);
  if (updatePhase) {
    return (
      <span className="flex items-center gap-1.5 h-6 px-2 rounded-md text-xs font-medium text-[var(--secondary-foreground)] border border-[var(--border)]">
        <Loader2 size={10} className="animate-spin" />
        {updatePhaseLabel(updatePhase.phase, updatePhase.version)}
      </span>
    );
  }
  if (installing) {
    return (
      <span className="flex items-center gap-1.5 h-6 px-2 rounded-md text-xs font-medium text-[var(--secondary-foreground)] border border-[var(--border)] tabular-nums">
        <Loader2 size={10} className="animate-spin" />
        {pct !== null ? `${Math.round(pct)}%` : "Installing…"}
      </span>
    );
  }
  if (state === "installed") {
    const remove = (
      <button
        onClick={() => {
          // Not `window.confirm`: WebView2 answers it `true` with no dialog,
          // so on Windows Remove fired unconfirmed.
          void useRemoveAgentConfirmStore
            .getState()
            .actions.ask({ name: entry.name })
            .then((ok) => {
              if (ok) onUninstall(entry);
            });
        }}
        className="h-6 px-2.5 rounded-md text-xs font-medium text-[var(--secondary-foreground)] border border-[var(--border)] hover:bg-[var(--atlas-element-hover)] hover:text-[var(--foreground)] transition-colors cursor-pointer"
      >
        Remove
      </button>
    );
    if (!entry.updateAvailable) return remove;
    return (
      <span className="flex items-center gap-1.5">
        <button
          onClick={() => onUpdate(entry)}
          title={`Install v${entry.version}. Waits for any reply in progress to finish; open chats continue on the new version with your next message.`}
          className="flex items-center gap-1 h-6 px-2.5 rounded-md text-xs font-medium text-[var(--foreground)] border border-[var(--border)] bg-[var(--card,var(--background))] hover:bg-[var(--atlas-element-hover)] transition-colors cursor-pointer"
        >
          <ArrowUpCircle size={10} />
          Update
        </button>
        {remove}
      </span>
    );
  }
  if (state === "detected") {
    // Atlas found this on the user's PATH but has NOT installed it, and a
    // detection alone never makes an agent spawnable (ADR-0002). The
    // badge says why the card is here; the button is the user accepting it,
    // which writes an installed-agents-map entry pointing at the copy they already
    // have — no download, and their binary is what runs.
    return (
      <span className="flex items-center gap-1.5">
        <span
          className="flex items-center gap-1 h-6 px-2 rounded-md text-xs font-medium text-[var(--muted-foreground)] border border-[var(--border)]"
          title={
            catalog?.resolvedPath ? `Found at ${catalog.resolvedPath}` : "Found on your system"
          }
        >
          <Check size={10} />
          Detected
        </span>
        <button
          onClick={() => onInstall(entry, kind)}
          title={
            catalog?.resolvedPath
              ? `Add it, running your own copy at ${catalog.resolvedPath}. Nothing is downloaded.`
              : "Add it, running the copy already on your system. Nothing is downloaded."
          }
          className="flex items-center gap-1 h-6 px-2.5 rounded-md text-xs font-medium text-[var(--foreground)] border border-[var(--border)] bg-[var(--card,var(--background))] hover:bg-[var(--atlas-element-hover)] transition-colors cursor-pointer"
        >
          Install
        </button>
      </span>
    );
  }
  return (
    <button
      onClick={() => onInstall(entry, kind)}
      disabled={!entry.platformSupported}
      className={cn(
        "flex items-center gap-1 h-6 px-2.5 rounded-md text-xs font-medium border transition-colors",
        entry.platformSupported
          ? "text-[var(--foreground)] border-[var(--border)] bg-[var(--card,var(--background))] hover:bg-[var(--atlas-element-hover)] cursor-pointer"
          : "text-[var(--muted-foreground)] border-[var(--border)] opacity-50 cursor-not-allowed",
      )}
    >
      <Download size={10} />
      Install
    </button>
  );
}
