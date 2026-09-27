import { useState, useEffect, useRef } from "react";
import { Dialog } from "@base-ui/react/dialog";
import { cn } from "@/lib/utils";
import { Hint } from "@/ui/tooltip";
import { invoke } from "@tauri-apps/api/core";
import { useExplorerStore } from "@/features/explorer/stores/explorer-store";
import { openFile } from "@/lib/open-file";
import { useSessionStore } from "@/features/app/stores/session-store";
import { useAppStore } from "@/features/app/stores/app-store";
import { Search, FileCode, Clock, X } from "lucide-react";

export interface SearchResult {
  file_path: string;
  line: number;
  content: string;
  match_start: number;
  match_end: number;
}

export function SearchOverlay({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<SearchResult[]>([]);
  const [searching, setSearching] = useState(false);
  const [hasSearched, setHasSearched] = useState(false);
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const rootPath = useExplorerStore.use.rootPath();
  const session = useSessionStore.use.session();
  const { addSearchHistory, removeSearchHistory, clearSearchHistory, saveSession } =
    useSessionStore.use.actions();
  const currentProject = useAppStore.use.currentProject();

  useEffect(() => {
    if (!open) {
      setQuery("");
      setResults([]);
      setSelectedIndex(0);
      setHasSearched(false);
    }
  }, [open]);

  const performSearch = async (searchQuery: string) => {
    if (!searchQuery.trim() || !rootPath) return;
    setSearching(true);
    setHasSearched(true);
    try {
      const res = await invoke<SearchResult[]>("search_in_files", {
        path: rootPath,
        query: searchQuery.trim(),
        maxResults: 50,
      });
      setResults(res);
      setSelectedIndex(0);
      addSearchHistory(searchQuery.trim());
      if (currentProject) saveSession(currentProject.path);
    } catch {
      setResults([]);
    }
    setSearching(false);
  };

  const openResult = (result: SearchResult) => {
    const fullPath = rootPath ? `${rootPath}/${result.file_path}` : result.file_path;
    void openFile(fullPath, { reveal: { line: result.line } });
    onOpenChange(false);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((i) => Math.min(i + 1, results.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter") {
      e.preventDefault();
      if (results.length > 0 && results[selectedIndex]) {
        openResult(results[selectedIndex]);
      } else {
        performSearch(query);
      }
    }
  };

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 scrim z-overlay" />
        <Dialog.Popup
          className={cn(
            "fixed top-[15%] left-1/2 -translate-x-1/2",
            "w-[600px] max-h-[500px] rounded-xl overflow-hidden",
            "bg-[var(--card)] border border-[var(--border)]",
            "shadow-md",
            "flex flex-col",
            "z-modal",
          )}
          // Base UI's initialFocus replaces Radix's onOpenAutoFocus +
          // preventDefault + focus(): hand it the element to land on.
          initialFocus={inputRef}
        >
          <div className="flex items-center gap-2 px-4 h-[44px] shrink-0 border-b border-[var(--border)]">
            <Search size={14} className="text-[var(--muted-foreground)] shrink-0" />
            <input
              ref={inputRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Search in files..."
              className="flex-1 bg-transparent border-none outline-none text-sm text-[var(--foreground)] placeholder:text-[var(--muted-foreground)]"
            />
            {searching && (
              <span className="text-2xs text-[var(--muted-foreground)]">Searching...</span>
            )}
          </div>

          <div className="overflow-y-auto flex-1 py-1">
            {results.length === 0 && hasSearched && !searching && (
              <div className="px-4 py-6 text-center text-xs text-[var(--muted-foreground)]">
                No results found
              </div>
            )}
            {!query.trim() && !hasSearched && session.searchHistory.length > 0 && (
              <div className="py-1">
                <div className="flex items-center justify-between px-4 py-1">
                  <span className="text-2xs text-[var(--muted-foreground)] uppercase tracking-wide font-semibold">
                    Recent searches
                  </span>
                  <button
                    onClick={() => {
                      clearSearchHistory();
                      if (currentProject) saveSession(currentProject.path);
                    }}
                    className="text-3xs text-[var(--muted-foreground)] hover:text-[var(--secondary-foreground)] cursor-pointer"
                  >
                    Clear all
                  </button>
                </div>
                {session.searchHistory.slice(0, 8).map((q, i) => (
                  <div
                    key={`${q}-${i}`}
                    className="flex items-center px-4 py-1.5 hover:bg-[var(--atlas-element-hover)] group"
                  >
                    <button
                      onClick={() => {
                        setQuery(q);
                        performSearch(q);
                      }}
                      className="flex items-center gap-2 flex-1 min-w-0 text-left"
                    >
                      <Clock size={11} className="text-[var(--muted-foreground)] shrink-0" />
                      <span className="text-xs text-[var(--secondary-foreground)] font-mono truncate">
                        {q}
                      </span>
                    </button>
                    <Hint label="Remove from history">
                      <button
                        onClick={() => {
                          removeSearchHistory(q);
                          if (currentProject) saveSession(currentProject.path);
                        }}
                        className="opacity-0 group-hover:opacity-100 focus-visible:opacity-100 p-0.5 text-[var(--muted-foreground)] hover:text-[var(--foreground)] shrink-0"
                      >
                        <X size={9} />
                      </button>
                    </Hint>
                  </div>
                ))}
              </div>
            )}
            {!query.trim() && !hasSearched && session.searchHistory.length === 0 && (
              <div className="px-4 py-6 text-center text-xs text-[var(--muted-foreground)]">
                Type to search across all files
              </div>
            )}
            {results.map((result, i) => (
              <button
                key={`${result.file_path}:${result.line}:${i}`}
                onClick={() => openResult(result)}
                onMouseEnter={() => setSelectedIndex(i)}
                className={cn(
                  "w-full text-left px-4 py-1.5 transition-colors",
                  i === selectedIndex ? "bg-[var(--atlas-element-hover)]" : "",
                )}
              >
                <div className="flex items-center gap-2">
                  <FileCode size={12} className="text-[var(--muted-foreground)] shrink-0" />
                  <span className="text-xs text-[var(--primary)] font-mono truncate">
                    {result.file_path}
                  </span>
                  <span className="text-2xs text-[var(--muted-foreground)] font-mono shrink-0">
                    :{result.line}
                  </span>
                </div>
                <div className="ml-5 text-xs font-mono text-[var(--secondary-foreground)] truncate mt-0.5">
                  {result.content.trim()}
                </div>
              </button>
            ))}
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
