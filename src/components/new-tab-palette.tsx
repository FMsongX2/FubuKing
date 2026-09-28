// Modified by FubuKing from upstream Atlas (Apache-2.0).
import { useState, useMemo, useRef, useEffect, useLayoutEffect, type ElementType } from "react";
import { ActionKbd } from "@/features/keybindings/components/action-kbd";
import type { ActionId } from "@/features/keybindings/lib/actions";
import { Dialog } from "@base-ui/react/dialog";
import {
  Map,
  Terminal,
  Globe,
  Brain,
  Network,
  BrainCircuit,
  ScrollText,
  Gauge,
  BarChart3,
  Code,
  Settings,
  Search,
  GitCompare,
  type LucideProps,
} from "lucide-react";
import { useLayoutStore } from "@/features/layout/stores/layout-store";
import { openNewAgentChat } from "@/features/chat/lib/open-agent-session";
import { AtlasIcon } from "@/components/atlas-icon";
import { cn } from "@/lib/utils";
import type { TabType } from "@/lib/constants";

interface ModuleEntry {
  id: string;
  type: TabType;
  label: string;
  icon: ElementType<LucideProps>;
  actionId?: ActionId;
}

/**
 * Authoritative list of tab modules surfaced in the new-tab palette.
 * Mirrors the `+` dropdown's options but is keyboard-first and
 * filterable. Visual chrome matches `command-palette.tsx` so the two
 * palettes read as one consistent system.
 */
const MODULES: ModuleEntry[] = [
  {
    id: "chat",
    type: "chat",
    label: "New Agents Chat",
    icon: AtlasIcon as ElementType<LucideProps>,
    actionId: "tabs.newChat",
  },
  { id: "canvas", type: "canvas", label: "Spaces", icon: Map },
  {
    id: "terminal",
    type: "terminal",
    label: "New Terminal",
    icon: Terminal,
    actionId: "tabs.newTerminal",
  },
  { id: "knowledge", type: "knowledge", label: "Knowledge", icon: Brain },
  { id: "knowledge-graph", type: "knowledge-graph", label: "Knowledge Graph", icon: Network },
  { id: "memory", type: "memory", label: "Memory", icon: BrainCircuit },
  { id: "diff", type: "diff", label: "Git Diff", icon: GitCompare },
  { id: "browser", type: "browser", label: "Browser", icon: Globe },
  {
    id: "editor",
    type: "editor",
    label: "Untitled Editor",
    icon: Code,
    actionId: "tabs.newUntitled",
  },
  { id: "log", type: "log", label: "Log", icon: ScrollText },
  { id: "usage", type: "usage", label: "Usage", icon: Gauge, actionId: "usage.open" },
  { id: "quota", type: "quota", label: "Quota", icon: BarChart3, actionId: "quota.open" },
  { id: "settings", type: "settings", label: "Settings", icon: Settings, actionId: "app.settings" },
];

export function NewTabPalette({
  open,
  onOpenChange,
}: {
  open: boolean;
  onOpenChange: (open: boolean) => void;
}) {
  const [query, setQuery] = useState("");
  const [selectedIndex, setSelectedIndex] = useState(0);
  const inputRef = useRef<HTMLInputElement>(null);
  const listRef = useRef<HTMLDivElement>(null);
  const { addTab } = useLayoutStore.use.actions();

  useEffect(() => {
    if (open) {
      setQuery("");
      setSelectedIndex(0);
    }
  }, [open]);

  const items = useMemo(() => {
    const q = query.trim().toLowerCase();
    if (!q) return MODULES;
    return MODULES.filter(
      (m) => m.label.toLowerCase().includes(q) || m.type.toLowerCase().includes(q),
    );
  }, [query]);

  useEffect(() => {
    if (selectedIndex >= items.length) {
      setSelectedIndex(Math.max(0, items.length - 1));
    }
  }, [items.length, selectedIndex]);

  useLayoutEffect(() => {
    const el = listRef.current?.querySelector(
      `[data-idx="${selectedIndex}"]`,
    ) as HTMLElement | null;
    el?.scrollIntoView({ block: "nearest" });
  }, [selectedIndex]);

  const commit = (item: ModuleEntry) => {
    const ts = Date.now();
    if (item.type === "chat") {
      // Singleton agent chat — focus the existing chat tab (resetting it to a
      // fresh session) instead of opening a second one.
      openNewAgentChat();
    } else if (item.type === "editor") {
      addTab({
        id: `editor-untitled-${ts}`,
        type: "editor",
        title: "Untitled",
        closable: true,
        dirty: false,
        data: { filePath: `untitled:${ts}` },
      });
    } else if (item.type === "knowledge-graph") {
      addTab({
        id: "knowledge-graph",
        type: "knowledge-graph",
        title: "Graph",
        closable: true,
        dirty: false,
        data: {},
      });
    } else if (item.type === "settings") {
      addTab({
        id: "settings",
        type: "settings",
        title: "Settings",
        closable: true,
        dirty: false,
        data: {},
      });
    } else {
      addTab({
        id: `${item.type}-${ts}`,
        type: item.type,
        title: item.label,
        closable: true,
        dirty: false,
        data: {},
      });
    }
    onOpenChange(false);
  };

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === "ArrowDown") {
      e.preventDefault();
      setSelectedIndex((i) => Math.min(i + 1, items.length - 1));
    } else if (e.key === "ArrowUp") {
      e.preventDefault();
      setSelectedIndex((i) => Math.max(i - 1, 0));
    } else if (e.key === "Enter" && items[selectedIndex]) {
      e.preventDefault();
      commit(items[selectedIndex]);
    }
  };

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Backdrop className="fixed inset-0 scrim z-overlay" />
        <Dialog.Popup
          aria-describedby={undefined}
          className={cn(
            "fixed top-[20%] left-1/2 -translate-x-1/2 z-modal",
            "w-[520px] max-h-[400px] rounded-xl overflow-hidden",
            "bg-[var(--card)] border border-[var(--border)]",
            "shadow-md",
            "flex flex-col",
          )}
          // Base UI's initialFocus replaces Radix's onOpenAutoFocus +
          // preventDefault + focus(): hand it the element to land on.
          initialFocus={inputRef}
        >
          <Dialog.Title className="sr-only">Open module</Dialog.Title>
          <div className="flex items-center gap-2 px-4 h-[44px] shrink-0 border-b border-[var(--border)]">
            <Search size={14} className="text-[var(--muted-foreground)] shrink-0" />
            <input
              ref={inputRef}
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              onKeyDown={handleKeyDown}
              placeholder="Open a module..."
              className="flex-1 bg-transparent border-none outline-none text-sm text-[var(--foreground)] placeholder:text-[var(--muted-foreground)]"
            />
          </div>

          <div ref={listRef} className="overflow-y-auto flex-1 py-1">
            {items.length === 0 && (
              <div className="px-4 py-6 text-center text-xs text-[var(--muted-foreground)]">
                No modules match "{query}"
              </div>
            )}
            {items.map((item, i) => {
              const Icon = item.icon;
              const active = i === selectedIndex;
              return (
                <button
                  key={item.id}
                  data-idx={i}
                  onClick={() => commit(item)}
                  onMouseEnter={() => setSelectedIndex(i)}
                  className={cn(
                    "w-full flex items-center gap-3 px-4 h-[36px] text-left text-sm transition-colors",
                    active
                      ? "bg-[var(--atlas-element-hover)] text-[var(--foreground)]"
                      : "text-[var(--secondary-foreground)]",
                  )}
                >
                  <Icon size={14} className="shrink-0 text-[var(--muted-foreground)]" />
                  <span className="flex-1 truncate">{item.label}</span>
                  {item.actionId && <ActionKbd id={item.actionId} />}
                </button>
              );
            })}
          </div>
        </Dialog.Popup>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
