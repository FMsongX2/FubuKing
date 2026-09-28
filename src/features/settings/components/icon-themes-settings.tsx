// Modified by FubuKing from upstream Atlas (Apache-2.0).
import { useEffect, useMemo, useState } from "react";
import { Download, Loader2, Search, Trash2, X } from "lucide-react";
import { toast } from "sonner";
import { cn } from "@/lib/utils";
import { Hint } from "@/ui/tooltip";
import { ScrollArea } from "@/ui/scroll-area";
import { useSettingsStore } from "@/features/settings/stores/settings-store";
import { useIconThemeStore } from "@/features/icon-theme/stores/icon-theme-store";
import { IconThemePreview } from "@/features/icon-theme/components/icon-theme-preview";
import { appearanceForMode } from "@/features/theme/apply-theme";
import {
  installIconTheme,
  removeIconTheme,
  searchIconThemes,
  MINIMAL_ICON_THEME_ID,
  type OpenVsxIconTheme,
} from "@/features/icon-theme/lib/icon-theme-api";

/**
 * The icon-theme picker, beside the colour-theme picker (decisions 4 and 11).
 *
 * Two halves. The top is what is installed — the bundled Material theme,
 * "Minimal" (Atlas's own lucide icons) and anything the user added. The bottom
 * is Open VSX, which decision 11 leaves as the only remote source.
 *
 * Nothing here fetches until a person types. A search that fails because the
 * machine is offline says so and leaves the installed list untouched: an icon
 * theme is an ornament, and losing the network must not cost anything already
 * chosen.
 */

export function IconThemesSettings() {
  const settings = useSettingsStore.use.settings();
  const { updateSettings } = useSettingsStore.use.actions();
  const themes = useIconThemeStore.use.themes();
  const loading = useIconThemeStore.use.loading();
  const error = useIconThemeStore.use.error();
  const { load } = useIconThemeStore.use.actions();

  useEffect(() => {
    void load();
  }, [load]);

  // The association set a theme would actually be resolved through, so a card
  // previews what applying it would give you. Icon themes have a third
  // ("highContrast") that Atlas never asks for; `appearanceForMode` is the one
  // `applyConfiguredIconTheme` is called with.
  const appearance = appearanceForMode(settings.themeMode);

  return (
    <div className="flex h-full min-h-0 flex-col">
      {/* The strip that used to live here showed the ACTIVE theme — the one
          theme nobody needs shown. Every card carries its own row now. */}
      <ScrollArea className="min-h-0 flex-1">
        <div className="p-2">
          <div className="grid grid-cols-2 gap-2">
            {themes.map((theme) => {
              const selected = theme.id === settings.iconTheme;
              return (
                <button
                  key={theme.id}
                  type="button"
                  onClick={() => {
                    updateSettings({ iconTheme: theme.id });
                    toast.success(`Applied “${theme.name}” icons`);
                  }}
                  className={cn(
                    "group flex min-h-20 cursor-pointer flex-col justify-between rounded-lg border bg-card p-3 text-left transition-colors",
                    selected ? "border-primary" : "border-border hover:border-border-strong",
                  )}
                >
                  <IconThemePreview
                    themeId={theme.id}
                    appearance={appearance}
                    className="mb-2.5 h-4"
                  />
                  <div className="flex w-full items-start gap-1.5">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-1.5">
                        <span className="truncate text-sm font-medium text-foreground">
                          {theme.name}
                        </span>
                        {selected && <span className="h-2 w-2 shrink-0 rounded-full bg-primary" />}
                      </div>
                      <p className="mt-1 truncate text-xs text-muted-foreground">
                        {theme.author} · {theme.license}
                      </p>
                    </div>
                    {!theme.builtIn && <RemoveButton id={theme.id} name={theme.name} />}
                  </div>
                  <div className="mt-2 flex items-center gap-1 text-3xs uppercase tracking-wide text-muted-foreground">
                    {theme.id === MINIMAL_ICON_THEME_ID ? (
                      <span>FubuKing defaults</span>
                    ) : theme.builtIn ? (
                      <span>Bundled</span>
                    ) : (
                      <span>Installed</span>
                    )}
                    {theme.warnings && theme.warnings.length > 0 && (
                      <Hint label={theme.warnings.map((w) => `${w.key}: ${w.message}`).join("\n")}>
                        <span className="text-warning">
                          {theme.warnings.length} warning{theme.warnings.length === 1 ? "" : "s"}
                        </span>
                      </Hint>
                    )}
                  </div>
                </button>
              );
            })}
          </div>

          {loading && (
            <div className="py-4 text-center text-xs text-muted-foreground">Loading…</div>
          )}
          {error && <div className="py-4 text-center text-xs text-error">{error}</div>}

          <OpenVsxSection />
        </div>
      </ScrollArea>
    </div>
  );
}

function RemoveButton({ id, name }: { id: string; name: string }) {
  const [busy, setBusy] = useState(false);
  const settings = useSettingsStore.use.settings();
  const { updateSettings } = useSettingsStore.use.actions();

  return (
    <Hint label={`Remove “${name}”`}>
      <span
        role="button"
        tabIndex={0}
        aria-label={`Remove ${name}`}
        onClick={(event) => {
          // The whole card is the "apply" button, so removing must not also
          // select what it is about to delete.
          event.stopPropagation();
          if (busy) return;
          setBusy(true);
          void removeIconTheme(id)
            .then(() => {
              // Falling back to the bundled default is the only safe landing:
              // the setting would otherwise name a directory that is gone.
              if (settings.iconTheme === id) updateSettings({ iconTheme: "material-icon-theme" });
              toast.success(`Removed “${name}”`);
            })
            .catch((reason: unknown) => toast.error(`Could not remove “${name}”: ${reason}`))
            .finally(() => setBusy(false));
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter" || event.key === " ") event.currentTarget.click();
        }}
        className="shrink-0 cursor-pointer rounded p-0.5 text-muted-foreground opacity-0 transition-opacity hover:text-error group-hover:opacity-100"
      >
        {busy ? <Loader2 size={11} className="animate-spin" /> : <Trash2 size={11} />}
      </span>
    </Hint>
  );
}

type SearchState =
  | { phase: "idle" }
  | { phase: "searching" }
  | { phase: "done"; results: OpenVsxIconTheme[] }
  | { phase: "failed"; message: string };

function OpenVsxSection() {
  const [query, setQuery] = useState("");
  const [state, setState] = useState<SearchState>({ phase: "idle" });
  const [installing, setInstalling] = useState<string | null>(null);
  const themes = useIconThemeStore.use.themes();
  const installedIds = useMemo(() => new Set(themes.map((theme) => theme.id)), [themes]);

  const run = () => {
    const trimmed = query.trim();
    if (!trimmed) {
      setState({ phase: "idle" });
      return;
    }
    setState({ phase: "searching" });
    void searchIconThemes(trimmed)
      .then((results) => setState({ phase: "done", results }))
      .catch((reason: unknown) => setState({ phase: "failed", message: String(reason) }));
  };

  const install = (hit: OpenVsxIconTheme) => {
    setInstalling(hit.id);
    void installIconTheme({ namespace: hit.namespace, name: hit.name })
      .then((summary) => toast.success(`Installed “${summary.name}”`))
      .catch((reason: unknown) => toast.error(`Could not install “${hit.displayName}”: ${reason}`))
      .finally(() => setInstalling(null));
  };

  return (
    <div className="mt-4 border-t border-border pt-3">
      <div className="mb-2 flex items-center justify-between">
        <span className="text-2xs font-semibold uppercase tracking-wider text-muted-foreground">
          Install from Open VSX
        </span>
        <span className="text-2xs text-muted-foreground">open-vsx.org</span>
      </div>

      <div className="flex h-[28px] items-center gap-1.5 rounded-md border border-border bg-card px-2">
        <Search size={11} className="shrink-0 text-muted-foreground" />
        <input
          value={query}
          onChange={(event) => setQuery(event.target.value)}
          onKeyDown={(event) => {
            if (event.key === "Enter") run();
          }}
          placeholder="Search VS Code icon themes, then press Enter…"
          spellCheck={false}
          className="min-w-0 flex-1 bg-transparent text-xs text-foreground outline-none placeholder:text-muted-foreground"
        />
        {query && (
          <Hint label="Clear">
            <button
              type="button"
              onClick={() => {
                setQuery("");
                setState({ phase: "idle" });
              }}
              className="shrink-0 cursor-pointer text-muted-foreground hover:text-foreground"
            >
              <X size={11} />
            </button>
          </Hint>
        )}
      </div>

      {state.phase === "searching" && (
        <div className="flex items-center justify-center gap-2 py-4 text-xs text-muted-foreground">
          <Loader2 size={11} className="animate-spin" />
          Searching Open VSX…
        </div>
      )}

      {state.phase === "failed" && (
        <div className="mt-2 rounded-md border border-error/40 bg-error/10 px-2 py-2 text-xs text-error">
          <p>{state.message}</p>
          <button
            type="button"
            onClick={run}
            className="mt-1 cursor-pointer underline underline-offset-2"
          >
            Try again
          </button>
        </div>
      )}

      {state.phase === "done" && state.results.length === 0 && (
        <div className="py-4 text-center text-xs text-muted-foreground">
          No icon themes on Open VSX match “{query}”.
        </div>
      )}

      {state.phase === "done" && state.results.length > 0 && (
        <div className="mt-2 flex flex-col gap-1">
          {state.results.map((hit) => {
            const already = hit.installed || installedIds.has(hit.id);
            const busy = installing === hit.id;
            return (
              <div
                key={hit.id}
                className="flex items-center gap-2 rounded-md border border-border bg-card px-2 py-1.5"
              >
                <div className="min-w-0 flex-1">
                  <div className="flex items-baseline gap-1.5">
                    <span className="truncate text-sm font-medium text-foreground">
                      {hit.displayName}
                    </span>
                    <span className="shrink-0 text-2xs text-muted-foreground">
                      {hit.namespace} · {hit.license}
                    </span>
                  </div>
                  <p className="truncate text-2xs text-muted-foreground">{hit.description}</p>
                </div>
                <button
                  type="button"
                  disabled={busy}
                  onClick={() => install(hit)}
                  className={cn(
                    "flex h-6 shrink-0 cursor-pointer items-center gap-1 rounded px-2 text-2xs font-medium transition-colors",
                    "border border-border text-secondary-foreground hover:bg-element-hover hover:text-foreground",
                    "disabled:cursor-not-allowed disabled:opacity-50",
                  )}
                >
                  {busy ? <Loader2 size={10} className="animate-spin" /> : <Download size={10} />}
                  {already ? "Reinstall" : "Install"}
                </button>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}
