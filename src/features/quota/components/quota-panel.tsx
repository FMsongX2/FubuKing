import { useEffect, useMemo, useState } from "react";
import { Clock, RefreshCw, Settings2 } from "lucide-react";
import { AgentIcons } from "@/components/agent-icons";
import { Button } from "@/ui/button";
import { Hint } from "@/ui/tooltip";
import { ScrollArea } from "@/ui/scroll-area";
import { cn } from "@/lib/utils";
import { timeAgo } from "@/lib/time-ago";
import { openSettingsSection } from "@/features/settings/lib/open-settings";
import type { AccountProvider } from "@/features/accounts/lib/accounts-api";
import type { AccountQuota, QuotaWindow } from "../lib/quota-api";
import {
  LEVEL_FILL,
  LEVEL_TEXT,
  formatCountdown,
  formatPlan,
  quotaLevel,
  remainingPercent,
  worstLevel,
} from "../lib/quota-format";
import { useQuotaStore } from "../stores/quota-store";

/**
 * The Quota tab: every account's limits, laid out the way Quotio shows them.
 * Provider tabs across the top, one card per account, one bar per window.
 */

type Filter = "all" | AccountProvider;

const PROVIDERS: ReadonlyArray<{ id: AccountProvider; name: string }> = [
  { id: "claude", name: "Claude Code" },
  { id: "codex", name: "Codex" },
];

export function ProviderIcon({
  provider,
  size = 14,
}: {
  provider: AccountProvider;
  size?: number;
}) {
  const Icon = provider === "claude" ? AgentIcons.Claude : AgentIcons.Codex;
  return <Icon width={size} height={size} className="shrink-0" />;
}

/** Re-render every minute so countdowns and "updated" times stay honest. */
function useMinuteTick(): number {
  const [now, setNow] = useState(() => Date.now());
  useEffect(() => {
    const id = window.setInterval(() => setNow(Date.now()), 60_000);
    return () => window.clearInterval(id);
  }, []);
  return now;
}

function isoFromSeconds(seconds: number | null): string | null {
  return seconds === null ? null : new Date(seconds * 1000).toISOString();
}

export function QuotaPanel() {
  const quotas = useQuotaStore.use.quotas();
  const loaded = useQuotaStore.use.loaded();
  const refreshing = useQuotaStore.use.refreshing();
  const error = useQuotaStore.use.error();
  const { start, refresh } = useQuotaStore.use.actions();
  const [filter, setFilter] = useState<Filter>("all");
  const now = useMinuteTick();

  useEffect(() => start(), [start]);

  const visible = useMemo(
    () => (filter === "all" ? quotas : quotas.filter((q) => q.provider === filter)),
    [filter, quotas],
  );
  const newest = quotas.reduce<number | null>(
    (latest, q) =>
      q.updatedAt !== null && (latest === null || q.updatedAt > latest) ? q.updatedAt : latest,
    null,
  );

  return (
    <div className="flex h-full min-h-0 flex-col">
      <div className="flex items-center justify-between gap-3 border-b border-border-subtle px-5 py-3">
        <div className="min-w-0">
          <p className="text-sm font-medium text-foreground">Quota</p>
          <p className="text-2xs text-muted-foreground">
            {refreshing
              ? "Reading accounts…"
              : newest !== null
                ? `Updated ${timeAgo(isoFromSeconds(newest), { suffix: true })}`
                : "Limits for every Claude Code and Codex account"}
          </p>
        </div>
        <div className="flex shrink-0 items-center gap-1">
          <Button size="sm" variant="ghost" onClick={() => openSettingsSection("accounts")}>
            <Settings2 size={12} />
            Manage accounts
          </Button>
          <Hint label="Refresh">
            <Button
              size="sm"
              variant="ghost"
              aria-label="Refresh"
              disabled={refreshing}
              onClick={() => void refresh()}
            >
              <RefreshCw size={12} className={cn(refreshing && "animate-spin")} />
            </Button>
          </Hint>
        </div>
      </div>

      <div className="flex items-center gap-1 px-5 pt-3">
        <FilterTab
          active={filter === "all"}
          label="All"
          count={quotas.length}
          onClick={() => setFilter("all")}
        />
        {PROVIDERS.map((provider) => {
          const accounts = quotas.filter((q) => q.provider === provider.id);
          if (accounts.length === 0) return null;
          return (
            <FilterTab
              key={provider.id}
              active={filter === provider.id}
              label={provider.name}
              icon={<ProviderIcon provider={provider.id} size={12} />}
              count={accounts.length}
              level={worstLevel(accounts)}
              onClick={() => setFilter(provider.id)}
            />
          );
        })}
      </div>

      <ScrollArea className="min-h-0 flex-1">
        <div className="flex flex-col gap-3 px-5 py-4">
          {error && <p className="text-2xs text-destructive">{error}</p>}
          {loaded && visible.length === 0 && (
            <div className="rounded-lg border border-border-subtle bg-card p-4">
              <p className="text-xs text-foreground">No accounts to read yet.</p>
              <p className="mt-1 text-2xs text-muted-foreground">
                Install the Claude Code or Codex CLI, or add an account in Settings → Accounts.
              </p>
            </div>
          )}
          {visible.map((account) => (
            <AccountCard key={account.accountId} account={account} now={now} />
          ))}
        </div>
      </ScrollArea>
    </div>
  );
}

function FilterTab({
  active,
  label,
  icon,
  count,
  level,
  onClick,
}: {
  active: boolean;
  label: string;
  icon?: React.ReactNode;
  count: number;
  level?: ReturnType<typeof worstLevel>;
  onClick: () => void;
}) {
  return (
    <Button
      size="sm"
      variant={active ? "secondary" : "ghost"}
      aria-pressed={active}
      onClick={onClick}
    >
      {icon}
      {label}
      <span className="rounded-full bg-element-hover px-1.5 text-2xs text-secondary-foreground">
        {count}
      </span>
      {level && <span className={cn("size-1.5 rounded-full", LEVEL_FILL[level])} aria-hidden />}
    </Button>
  );
}

function AccountCard({ account, now }: { account: AccountQuota; now: number }) {
  const plan = formatPlan(account.plan);
  return (
    <div className="rounded-lg border border-border-subtle bg-card p-4">
      <div className="flex items-center justify-between gap-3">
        <div className="flex min-w-0 items-center gap-2">
          <ProviderIcon provider={account.provider} />
          <p className="truncate text-xs font-medium text-foreground">{account.displayName}</p>
          {account.isDefault && (
            <span className="rounded-full bg-element-hover px-1.5 text-2xs text-secondary-foreground">
              default
            </span>
          )}
          {plan && (
            <span className="rounded-full bg-info-muted px-1.5 text-2xs text-info">{plan}</span>
          )}
        </div>
        {account.status === "ok" && account.updatedAt !== null && (
          <span className="shrink-0 text-2xs text-muted-foreground">
            {timeAgo(isoFromSeconds(account.updatedAt), { suffix: true })}
          </span>
        )}
      </div>

      {account.status === "ok" && account.windows.length > 0 ? (
        <div className="mt-3 flex flex-col gap-3">
          {account.windows.map((window) => (
            <WindowRow key={window.id} window={window} now={now} />
          ))}
        </div>
      ) : (
        <StatusNote account={account} />
      )}
    </div>
  );
}

function WindowRow({ window, now }: { window: QuotaWindow; now: number }) {
  const left = remainingPercent(window);
  const level = quotaLevel(left);
  const countdown = formatCountdown(window.resetsAt, now);
  return (
    <div className="flex flex-col gap-1.5">
      <div className="flex items-center justify-between gap-2">
        <span className="truncate text-xs text-foreground">{window.label}</span>
        <div className="flex shrink-0 items-center gap-2">
          <span className={cn("text-xs font-medium tabular-nums", LEVEL_TEXT[level])}>
            {left}% left
          </span>
          {countdown && (
            <Hint label="Resets in">
              <span className="flex items-center gap-1 rounded-full bg-element-hover px-1.5 text-2xs tabular-nums text-secondary-foreground">
                <Clock size={10} />
                {countdown}
              </span>
            </Hint>
          )}
        </div>
      </div>
      <div
        className="h-1.5 overflow-hidden rounded-full bg-border-subtle"
        role="progressbar"
        aria-label={`${window.label}: ${left}% left`}
        aria-valuemin={0}
        aria-valuemax={100}
        aria-valuenow={left}
      >
        <div
          className={cn("h-full rounded-full", LEVEL_FILL[level])}
          style={{ width: `${left}%` }}
        />
      </div>
    </div>
  );
}

function StatusNote({ account }: { account: AccountQuota }) {
  const text =
    account.message ??
    (account.status === "ok" ? "This account reports no usage limits." : "No figures yet.");
  return (
    <div className="mt-3 flex items-center justify-between gap-3">
      <p className="text-2xs text-muted-foreground">{text}</p>
      {account.status === "untracked" && (
        <Button size="xs" variant="outline" onClick={() => openSettingsSection("accounts")}>
          Add account
        </Button>
      )}
    </div>
  );
}
