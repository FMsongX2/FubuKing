// Modified by FubuKing from upstream Atlas (Apache-2.0).
import { Fragment, useEffect, useMemo, useRef } from "react";
import { AlertTriangle, Pencil } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { KbdKeys } from "@/ui/kbd";
import { Hint, Tooltip, TooltipContent, TooltipTrigger } from "@/ui/tooltip";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuShortcut,
  ContextMenuTrigger,
} from "@/ui/context-menu";
import { copyText } from "@/lib/clipboard";
import { ACTION_BY_ID, type ActionDef, type ActionId, WHEN_LABELS } from "../lib/actions";
import { type Combo, displayKeys, displayLabel } from "../lib/combo";
import type { Conflict, ResolvedBinding } from "../lib/resolve";
import { useKeybindingsStore } from "../stores/keybindings-store";
import type { RecorderMode } from "./keybinding-recorder";

export interface TableRow {
  def: ActionDef;
  id: ActionId;
  bindings: ResolvedBinding[];
  overridden: boolean;
  invalid: string[];
}

export type RowGroup = { title: string; rows: TableRow[] };

const GRID =
  "grid grid-cols-[28px_minmax(0,2.2fr)_minmax(0,1.8fr)_minmax(0,1.1fr)_72px] items-center";

/**
 * The Command / Keybinding / When / Source table. Plain rows (≈55), no
 * virtualisation. The container owns keyboard navigation: ↑↓ move the
 * selection, Enter opens the recorder, ⌫/⌦ removes, ⌘C copies.
 */
export function KeybindingsTable({
  groups,
  selectedId,
  onSelect,
  onRecord,
  conflicts,
  onShowSame,
  unknownIds,
  emptyHint,
}: {
  groups: RowGroup[];
  selectedId: ActionId | null;
  onSelect: (id: ActionId | null) => void;
  onRecord: (id: ActionId, mode: RecorderMode) => void;
  conflicts: Map<string, Conflict>;
  onShowSame: (combo: Combo) => void;
  unknownIds: string[];
  emptyHint: string | null;
}) {
  const { removeBinding, resetBinding, removeUnknown } = useKeybindingsStore.use.actions();
  const recording = useKeybindingsStore.use.recording();
  const locked = useKeybindingsStore(
    (s) => !!s.file.profiles.find((p) => p.id === s.file.activeProfileId)?.builtIn,
  );
  const containerRef = useRef<HTMLDivElement>(null);
  const flat = useMemo(() => groups.flatMap((g) => g.rows), [groups]);

  // Keep the selected row in view when it changes via the keyboard.
  useEffect(() => {
    if (!selectedId) return;
    containerRef.current
      ?.querySelector<HTMLElement>(`[data-action-id="${CSS.escape(selectedId)}"]`)
      ?.scrollIntoView({ block: "nearest" });
  }, [selectedId]);

  const onKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (recording) return;
    const idx = selectedId ? flat.findIndex((r) => r.id === selectedId) : -1;
    if (e.key === "ArrowDown" || e.key === "ArrowUp") {
      e.preventDefault();
      if (flat.length === 0) return;
      const next =
        e.key === "ArrowDown" ? Math.min(flat.length - 1, idx + 1) : Math.max(0, idx - 1);
      onSelect(flat[next]!.id);
      return;
    }
    if (idx < 0) return;
    const row = flat[idx]!;
    if (e.key === "Enter") {
      e.preventDefault();
      onRecord(row.id, "change");
    } else if (e.key === "Backspace" || e.key === "Delete") {
      e.preventDefault();
      if (!locked) removeBinding(row.id);
    } else if (e.key === "Escape") {
      onSelect(null);
    } else if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "c") {
      e.preventDefault();
      void copyRow(row);
    }
    e.stopPropagation();
  };

  return (
    <div
      ref={containerRef}
      tabIndex={0}
      onKeyDown={onKeyDown}
      className="min-h-0 flex-1 overflow-y-auto outline-none"
      style={{ scrollbarWidth: "none" }}
    >
      <div
        className={cn(
          GRID,
          "sticky top-0 z-10 h-[26px] border-b border-border bg-background px-2",
          "text-2xs font-semibold uppercase tracking-wider text-muted-foreground",
        )}
      >
        <span />
        <span>Command</span>
        <span>Keybinding</span>
        <span>When</span>
        <span>Source</span>
      </div>

      {flat.length === 0 && (
        <div className="flex h-24 items-center justify-center text-xs text-muted-foreground">
          {emptyHint ?? "No matching keybindings"}
        </div>
      )}

      {groups.map((g) => (
        <Fragment key={g.title}>
          {g.title && (
            <div className="px-3 pt-2.5 pb-1 text-2xs uppercase tracking-wider text-muted-foreground">
              {g.title}
            </div>
          )}
          {g.rows.map((row) => (
            <Row
              key={row.id}
              row={row}
              selected={row.id === selectedId}
              locked={locked}
              conflicts={conflicts}
              onSelect={() => onSelect(row.id)}
              onRecord={(mode) => onRecord(row.id, mode)}
              onRemove={(combo) => removeBinding(row.id, combo)}
              onReset={() => resetBinding(row.id)}
              onShowSame={onShowSame}
              onCopy={() => void copyRow(row)}
            />
          ))}
        </Fragment>
      ))}

      {unknownIds.length > 0 && (
        <>
          <div className="px-3 pt-3 pb-1 text-2xs uppercase tracking-wider text-muted-foreground">
            Unknown commands
          </div>
          {unknownIds.map((id) => (
            <div key={id} className={cn(GRID, "h-[28px] px-2 text-xs text-muted-foreground")}>
              <span />
              <span className="truncate font-mono text-xs">{id}</span>
              <span className="text-muted-foreground">not in this version of FubuKing</span>
              <span />
              <button
                type="button"
                onClick={() => removeUnknown(id)}
                disabled={locked}
                className="text-xs text-secondary-foreground hover:text-foreground disabled:opacity-40 cursor-pointer text-left"
              >
                Remove
              </button>
            </div>
          ))}
        </>
      )}
      <div className="h-4" />
    </div>
  );
}

async function copyRow(row: TableRow) {
  const label = row.bindings.map((b) => displayLabel(b.combo)).join(", ") || "unbound";
  const ok = await copyText(`${row.def.title} — ${label} (${row.id})`);
  if (!ok) toast.error("Couldn't copy");
}

function Row({
  row,
  selected,
  locked,
  conflicts,
  onSelect,
  onRecord,
  onRemove,
  onReset,
  onShowSame,
  onCopy,
}: {
  row: TableRow;
  selected: boolean;
  locked: boolean;
  conflicts: Map<string, Conflict>;
  onSelect: () => void;
  onRecord: (mode: RecorderMode) => void;
  onRemove: (combo?: string) => void;
  onReset: () => void;
  onShowSame: (combo: Combo) => void;
  onCopy: () => void;
}) {
  const source = row.overridden ? "User" : "Default";
  // Per-row severity: red when another action in the SAME context shares a
  // chord (it can never fire); amber when a user override overlaps a chord
  // from another context. Shipped default overlaps across contexts (the
  // terminal's ⌘W shadowing close-tab) are by design and stay quiet.
  const others = row.bindings
    .flatMap((b) => conflicts.get(b.serialized)?.bindings ?? [])
    .filter((b) => b.actionId !== row.id);
  const hardOthers = others.filter((o) => o.when === row.def.when);
  const softOthers = others.filter(
    (o) => o.when !== row.def.when && (row.overridden || o.source === "user"),
  );
  const worst = hardOthers.length ? "hard" : softOthers.length ? "soft" : null;
  const shown = worst === "hard" ? hardOthers : softOthers;
  const firstCombo = row.bindings[0]?.combo ?? null;

  return (
    <ContextMenu onOpenChange={(open) => open && onSelect()}>
      <ContextMenuTrigger
        render={
          <div
            data-action-id={row.id}
            onClick={onSelect}
            onDoubleClick={() => onRecord("change")}
            className={cn(
              GRID,
              "group h-[28px] px-2 text-xs border-b border-border-subtle cursor-default select-none",
              selected ? "bg-element-selected" : "hover:bg-element-hover",
            )}
          >
            <Hint label="Change keybinding">
              <button
                type="button"
                onClick={(e) => {
                  e.stopPropagation();
                  onRecord("change");
                }}
                className={cn(
                  "flex h-5 w-5 items-center justify-center rounded text-muted-foreground hover:text-foreground transition-opacity cursor-pointer",
                  selected
                    ? "opacity-100"
                    : "opacity-0 group-hover:opacity-100 focus-visible:opacity-100",
                )}
              >
                <Pencil size={11} />
              </button>
            </Hint>
            <div className="flex min-w-0 items-baseline gap-2">
              <span
                className={cn(
                  "truncate",
                  selected
                    ? "text-foreground"
                    : "text-secondary-foreground group-hover:text-foreground",
                )}
              >
                {row.def.title}
              </span>
              <span className="hidden truncate font-mono text-2xs text-muted-foreground @[640px]:inline">
                {row.id}
              </span>
            </div>
            <div className="flex min-w-0 items-center gap-2">
              {row.bindings.length === 0 ? (
                <span className="text-muted-foreground">—</span>
              ) : (
                row.bindings.map((b) => <KbdKeys key={b.serialized} keys={displayKeys(b.combo)} />)
              )}
              {row.invalid.length > 0 && (
                <Tooltip>
                  <TooltipTrigger
                    render={
                      <AlertTriangle
                        size={11}
                        className="shrink-0 text-[var(--atlas-status-error-foreground)]"
                      />
                    }
                  />
                  <TooltipContent>
                    Invalid in keybindings.json: {row.invalid.join(", ")}
                  </TooltipContent>
                </Tooltip>
              )}
              {worst && (
                <Tooltip>
                  <TooltipTrigger
                    render={
                      <button
                        type="button"
                        aria-label="Show conflicting bindings"
                        onClick={(e) => {
                          e.stopPropagation();
                          if (firstCombo) onShowSame(firstCombo);
                        }}
                        className="flex shrink-0 cursor-pointer"
                      >
                        <AlertTriangle
                          size={11}
                          className={
                            worst === "hard"
                              ? "text-[var(--atlas-status-error-foreground)]"
                              : "text-[var(--atlas-status-warning-foreground)]"
                          }
                        />
                      </button>
                    }
                  />
                  <TooltipContent>
                    {worst === "hard"
                      ? "Also bound in the same context: "
                      : "Also bound elsewhere: "}
                    {[...new Set(shown.map((b) => ACTION_BY_ID[b.actionId].title))].join(", ")}
                  </TooltipContent>
                </Tooltip>
              )}
            </div>
            <span className="truncate font-mono text-2xs text-muted-foreground">
              {WHEN_LABELS[row.def.when] || <span className="text-muted-foreground">—</span>}
            </span>
            <span
              className={cn(
                "text-xs",
                row.overridden ? "text-foreground" : "text-muted-foreground",
              )}
            >
              {source}
            </span>
          </div>
        }
      />
      <ContextMenuContent>
        <ContextMenuItem onClick={onCopy}>
          Copy
          <ContextMenuShortcut>⌘C</ContextMenuShortcut>
        </ContextMenuItem>
        <ContextMenuItem onClick={() => void copyText(row.id)}>Copy Command ID</ContextMenuItem>
        <ContextMenuItem onClick={() => void copyText(row.def.title)}>
          Copy Command Title
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem onClick={() => onRecord("change")}>
          Change Keybinding…
          <ContextMenuShortcut>↩</ContextMenuShortcut>
        </ContextMenuItem>
        <ContextMenuItem onClick={() => onRecord("add")}>Add Keybinding…</ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem disabled={locked || row.bindings.length === 0} onClick={() => onRemove()}>
          Remove Keybinding
          <ContextMenuShortcut>⌫</ContextMenuShortcut>
        </ContextMenuItem>
        {row.bindings.length > 1 &&
          row.bindings.map((b) => (
            <ContextMenuItem
              key={b.serialized}
              inset
              disabled={locked}
              onClick={() => onRemove(b.serialized)}
            >
              Remove {displayLabel(b.combo)}
            </ContextMenuItem>
          ))}
        <ContextMenuItem disabled={locked || !row.overridden} onClick={onReset}>
          Reset Keybinding
        </ContextMenuItem>
        <ContextMenuSeparator />
        <ContextMenuItem
          disabled={!firstCombo}
          onClick={() => firstCombo && onShowSame(firstCombo)}
        >
          Show Same Keybindings
        </ContextMenuItem>
      </ContextMenuContent>
    </ContextMenu>
  );
}
