// Persisted provider+model preference for the native Atlas agent.
//
// The ACP agents (Claude Code / Codex) carry their model server-side, but the
// in-process native agent picks a BYOK provider+model in the composer. New chats
// start fresh, so without this the picker would reset to the first configured
// provider every time. We remember the last full selection (globally, not per
// project — it's a user preference) and seed new sessions from it.

export interface NativeModelPref {
  provider: string;
  model: string;
}

const KEY = "atlas:agent-model-pref";

/** Last provider+model the user picked for the native agent, or null. */
export function loadNativeModelPref(): NativeModelPref | null {
  try {
    const raw = localStorage.getItem(KEY);
    if (!raw) return null;
    const v = JSON.parse(raw) as NativeModelPref;
    if (
      v &&
      typeof v.provider === "string" &&
      typeof v.model === "string" &&
      v.provider &&
      v.model
    ) {
      return v;
    }
  } catch {
    // corrupt / unavailable storage — treat as a miss
  }
  return null;
}

/** Persist the user's provider+model selection (best-effort). */
export function saveNativeModelPref(pref: NativeModelPref): void {
  try {
    if (pref.provider && pref.model) {
      localStorage.setItem(KEY, JSON.stringify(pref));
    }
  } catch {
    // storage full / unavailable — best-effort
  }
}

const EFFORT_KEY = "atlas:agent-effort-pref";

/** Last reasoning-effort level the user picked ("" = model default), or "". */
export function loadNativeEffort(): string {
  try {
    return localStorage.getItem(EFFORT_KEY) ?? "";
  } catch {
    return "";
  }
}

/** Persist the reasoning-effort preference (best-effort). */
export function saveNativeEffort(effort: string): void {
  try {
    localStorage.setItem(EFFORT_KEY, effort);
  } catch {
    // best-effort
  }
}
