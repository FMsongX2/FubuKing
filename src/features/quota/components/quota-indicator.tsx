import { useEffect, useMemo } from "react";
import { useLayoutStore } from "@/features/layout/stores/layout-store";
import { Hint } from "@/ui/tooltip";
import { cn } from "@/lib/utils";
import type { AccountProvider } from "@/features/accounts/lib/accounts-api";
import { LEVEL_TEXT, quotaLevel, remainingPercent, tightestWindow } from "../lib/quota-format";
import { useQuotaStore } from "../stores/quota-store";
import { ProviderIcon } from "./quota-panel";

/**
 * The title bar's quota readout, the in-window counterpart of Quotio's menu
 * bar item: per provider, the least any of its accounts has left in its
 * tightest window. Clicking opens the Quota tab. Hidden until some account
 * has figures.
 */
export function QuotaIndicator() {
  const quotas = useQuotaStore.use.quotas();
  const { start } = useQuotaStore.use.actions();
  const { addTab } = useLayoutStore.use.actions();

  useEffect(() => start(), [start]);

  const readings = useMemo(() => {
    const byProvider = new Map<AccountProvider, number>();
    for (const account of quotas) {
      const window = account.status === "ok" ? tightestWindow(account) : null;
      if (!window) continue;
      const left = remainingPercent(window);
      const current = byProvider.get(account.provider);
      if (current === undefined || left < current) byProvider.set(account.provider, left);
    }
    return [...byProvider.entries()];
  }, [quotas]);

  if (readings.length === 0) return null;

  const openQuota = () =>
    addTab({ id: "quota", type: "quota", title: "Quota", closable: true, dirty: false, data: {} });

  return (
    <Hint label="Quota left: the tightest window per provider">
      <button
        type="button"
        onClick={openQuota}
        className="mr-1 flex h-control-sm items-center gap-2 rounded px-2 text-2xs tabular-nums transition-colors hover:bg-element-hover"
        aria-label="Open quota"
      >
        {readings.map(([provider, left]) => (
          <span key={provider} className="flex items-center gap-1">
            <ProviderIcon provider={provider} size={12} />
            <span className={cn("font-medium", LEVEL_TEXT[quotaLevel(left)])}>{left}%</span>
          </span>
        ))}
      </button>
    </Hint>
  );
}
