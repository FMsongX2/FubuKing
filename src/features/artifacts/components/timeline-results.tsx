/**
 * What the right pane shows while the board is narrowed.
 *
 * A search or a filter is a different question from "which session do I want to
 * read next", and the nav answers badly: its rows carry a title and nothing
 * else, so twelve results that all mention "chat" look identical. Results get a
 * table instead — the same shape as the API-keys table, an uppercase column
 * header over hairline-separated rows — because comparing is the whole task and
 * comparing wants columns.
 *
 * Windowed like the nav. A query matching a common word can return every row
 * the board holds, and a table of 500 sessions is 3,000 cells.
 */

import { useRef } from "react";
import { useVirtualizer } from "@tanstack/react-virtual";
import { ChevronRight, X } from "lucide-react";

import { timeAgo } from "@/lib/time-ago";
import { cn } from "@/lib/utils";
import { Hint } from "@/ui/tooltip";

import {
  formatDuration,
  formatTokens,
  prettyModel,
  sessionState,
  sessionTitle,
  type FacetKey,
  type FacetSelection,
} from "../lib/board";
import type { BoardSession } from "../types";
import { AgentGlyph } from "./agent-glyph";

/** Row height. Taller than the nav's — these rows carry six fields. */
const ROW_H = 40;

/**
 * The grid every row and the header share.
 *
 * One constant, because a header that drifts from its rows by a pixel is worse
 * than no header at all.
 *
 * The fixed columns are sized to their widest realistic value and no more: the
 * pane can be as narrow as the nav leaves it, and every pixel they take comes
 * out of the title — which is the column people actually read. The first pass
 * gave them 496px plus gaps and left the title truncating at four characters.
 */
const GRID =
  "grid grid-cols-[minmax(0,1fr)_104px_88px_64px_56px_64px_14px] items-center gap-3 px-4";

export function TimelineResults({
  sessions,
  query,
  selection,
  projectFilter,
  projectName,
  onClearQuery,
  onClearFacet,
  onClearProject,
  onOpen,
}: {
  /** Already narrowed by the panel — this component only draws. */
  sessions: BoardSession[];
  query: string;
  selection: FacetSelection;
  projectFilter: string | null;
  /** Display name for `projectFilter`, which is a path. */
  projectName: string | null;
  onClearQuery: () => void;
  onClearFacet: (key: FacetKey) => void;
  onClearProject: () => void;
  onOpen: (id: string, projectPath: string, remoteProjectId: string | null) => void;
}) {
  const parentRef = useRef<HTMLDivElement | null>(null);
  const virtualizer = useVirtualizer({
    count: sessions.length,
    getScrollElement: () => parentRef.current,
    estimateSize: () => ROW_H,
    overscan: 12,
    getItemKey: (i) => sessions[i]?.id ?? i,
  });

  const facetChips = (Object.keys(selection) as FacetKey[])
    .filter((key) => selection[key] !== null)
    .map((key) => ({ key, value: selection[key]! }));

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* What is being asked, and every way to stop asking it. A results page
          that does not state its own terms leaves you guessing why a session
          you know exists is missing. */}
      <div className="flex shrink-0 flex-wrap items-center gap-2 px-4 pb-3 pt-3.5">
        <span className="text-base text-[var(--foreground)]">
          {sessions.length} {sessions.length === 1 ? "result" : "results"}
        </span>
        {query && <Chip label={`“${query}”`} onClear={onClearQuery} />}
        {projectFilter && (
          <Chip label={projectName ?? projectFilter} field="project" onClear={onClearProject} />
        )}
        {facetChips.map((chip) => (
          <Chip
            key={chip.key}
            label={chip.value}
            field={chip.key}
            onClear={() => onClearFacet(chip.key)}
          />
        ))}
      </div>

      <div
        className={cn(
          GRID,
          "h-7 shrink-0 border-y border-[var(--atlas-border-subtle)] font-mono text-2xs uppercase tracking-[0.08em] text-[var(--muted-foreground)]",
        )}
      >
        <span>Session</span>
        <span>Project</span>
        <span>Model</span>
        <span className="text-right">Tokens</span>
        <span className="text-right">Time</span>
        {/* "Active", not "Last active": the longer label wrapped to two lines
            in its own column and pushed the header row out of alignment. */}
        <span className="text-right">Active</span>
        <span />
      </div>

      {sessions.length === 0 ? (
        <p className="px-4 py-10 text-center text-sm text-[var(--muted-foreground)]">
          Nothing matches. Try a shorter term, or clear a filter above.
        </p>
      ) : (
        <div ref={parentRef} className="hide-scrollbar min-h-0 flex-1 overflow-y-auto">
          <div style={{ height: virtualizer.getTotalSize(), position: "relative", width: "100%" }}>
            {virtualizer.getVirtualItems().map((v) => {
              const session = sessions[v.index];
              return (
                <div
                  key={session.id}
                  style={{
                    position: "absolute",
                    top: 0,
                    left: 0,
                    width: "100%",
                    height: ROW_H,
                    transform: `translateY(${v.start}px)`,
                  }}
                >
                  <Row session={session} onOpen={onOpen} />
                </div>
              );
            })}
          </div>
        </div>
      )}
    </div>
  );
}

function Row({
  session,
  onOpen,
}: {
  session: BoardSession;
  onOpen: (id: string, projectPath: string, remoteProjectId: string | null) => void;
}) {
  const title = sessionTitle(session.title);
  const live = sessionState(session) === "live";
  return (
    <button
      type="button"
      onClick={() => onOpen(session.id, session.projectPath, session.remoteProjectId)}
      title={title ?? undefined}
      className={cn(
        GRID,
        "h-full w-full cursor-pointer border-b border-[var(--atlas-border-subtle)] text-left transition-colors hover:bg-[var(--atlas-element-active)]",
      )}
    >
      <span className="flex min-w-0 items-center gap-2.5">
        {session.agent ? (
          <AgentGlyph agent={session.agent} mono />
        ) : (
          <span className="size-[11px]" />
        )}
        <span
          className={cn(
            "min-w-0 truncate text-base",
            title ? "text-[var(--foreground)]" : "text-[var(--muted-foreground)]",
          )}
        >
          {title ?? "Untitled session"}
        </span>
      </span>
      <span className="truncate text-sm text-[var(--secondary-foreground)]">
        {session.projectName}
      </span>
      <span className="truncate font-mono text-xs text-[var(--muted-foreground)]">
        {prettyModel(session.model) ?? "—"}
      </span>
      <span className="text-right font-mono text-xs tabular-nums text-[var(--muted-foreground)]">
        {session.totalTokens > 0 ? formatTokens(session.totalTokens) : "—"}
      </span>
      <span
        className={cn(
          "text-right font-mono text-xs tabular-nums",
          live
            ? "text-[var(--atlas-status-success-foreground)]"
            : "text-[var(--secondary-foreground)]",
        )}
      >
        {formatDuration(session.activeSeconds)}
      </span>
      <span className="text-right font-mono text-xs tabular-nums text-[var(--atlas-text-disabled)]">
        {timeAgo(session.lastActivityAt)}
      </span>
      <ChevronRight size={13} className="text-[var(--atlas-text-disabled)]" />
    </button>
  );
}

/** One active term, with the way to drop it. */
function Chip({
  label,
  field,
  onClear,
}: {
  label: string;
  /** The facet this value belongs to — `agent`, `model`, … */
  field?: string;
  onClear: () => void;
}) {
  return (
    <span className="flex h-6 max-w-[220px] items-center gap-1.5 rounded-full border border-[var(--border)] bg-[var(--background)] pl-2.5 pr-1.5 text-xs text-[var(--secondary-foreground)]">
      {field && <span className="shrink-0 text-[var(--atlas-text-disabled)]">{field}</span>}
      <span className="min-w-0 truncate">{label}</span>
      <Hint label={`Clear ${field ?? "search"}`}>
        <button
          type="button"
          onClick={onClear}
          className="flex size-4 shrink-0 cursor-pointer items-center justify-center rounded-full text-[var(--muted-foreground)] transition-colors hover:bg-[var(--atlas-element-hover)] hover:text-[var(--foreground)]"
        >
          <X size={10} />
        </button>
      </Hint>
    </span>
  );
}
