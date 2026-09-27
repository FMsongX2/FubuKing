// Quota store: mirrors the Rust quota service's last snapshot. Rust reads the
// accounts on its own timer and broadcasts `atlas:quota-changed`; this store
// subscribes once and exposes a manual refresh.

import { create } from "zustand";
import { createSelectors } from "@/lib/create-selectors";
import { quota, type AccountQuota } from "../lib/quota-api";

interface QuotaState {
  quotas: AccountQuota[];
  /** False until the first snapshot or event has arrived. */
  loaded: boolean;
  refreshing: boolean;
  error: string | null;
  actions: {
    /** Load the snapshot and follow broadcasts. Safe to call from every view
     *  that needs quota; only the first call does anything. */
    start: () => void;
    refresh: () => Promise<void>;
  };
}

let started = false;

const useQuotaStoreBase = create<QuotaState>((set, get) => ({
  quotas: [],
  loaded: false,
  refreshing: false,
  error: null,
  actions: {
    start: () => {
      if (started) return;
      started = true;
      void quota.onChanged((quotas) => set({ quotas, loaded: true, error: null }));
      quota
        .snapshot()
        .then((quotas) => {
          // Before the service's first read the snapshot is empty; ask for
          // one rather than showing an empty view for the first few seconds.
          if (quotas.length === 0) void get().actions.refresh();
          else set({ quotas, loaded: true });
        })
        .catch((e) => set({ error: String(e) }));
    },
    refresh: async () => {
      if (get().refreshing) return;
      set({ refreshing: true });
      try {
        const quotas = await quota.refresh();
        set({ quotas, loaded: true, error: null });
      } catch (e) {
        set({ error: String(e) });
      } finally {
        set({ refreshing: false });
      }
    },
  },
}));

export const useQuotaStore = createSelectors(useQuotaStoreBase);
