// Modified by Quotatlas from upstream Atlas (Apache-2.0).
import { useEffect, useState } from "react";
import { Lock } from "lucide-react";
import { cn } from "@/lib/utils";
import { KbdKeys } from "@/ui/kbd";
import { ACTION_BY_ID, type ActionId } from "../lib/actions";
import { type Combo, comboFromEvent, displayKeys, serializeCombo } from "../lib/combo";
import { bindingsForCombo } from "../lib/resolve";
import { useKeybindingsStore } from "../stores/keybindings-store";

export type RecorderMode = "change" | "add";

/**
 * "Press desired key combination and then press ENTER." — the floating
 * recorder. While mounted it flips the store's `recording` flag so every
 * dispatcher in the app stays silent, and it consumes keydown in the capture
 * phase so nothing else sees the chord.
 *
 * On the locked Default profile it offers to duplicate instead of recording.
 */
export function KeybindingRecorder({
  actionId,
  mode,
  onClose,
  onShowSame,
}: {
  actionId: ActionId;
  mode: RecorderMode;
  onClose: () => void;
  onShowSame: (combo: Combo) => void;
}) {
  const file = useKeybindingsStore.use.file();
  const resolved = useKeybindingsStore.use.resolved();
  const { setRecording, setBinding, addBinding, duplicateProfile } =
    useKeybindingsStore.use.actions();
  const active = file.profiles.find((p) => p.id === file.activeProfileId);
  const locked = !!active?.builtIn;
  const def = ACTION_BY_ID[actionId];

  const [combo, setCombo] = useState<Combo | null>(null);
  // Modifiers held right now, for the live preview before a key lands.
  const [held, setHeld] = useState<string[]>([]);

  useEffect(() => {
    setRecording(true);
    return () => setRecording(false);
  }, [setRecording]);

  useEffect(() => {
    if (locked) return;
    const onKeyDown = (e: KeyboardEvent) => {
      e.preventDefault();
      e.stopImmediatePropagation();
      if (e.key === "Escape" && !e.metaKey && !e.altKey && !e.ctrlKey && !e.shiftKey) {
        onClose();
        return;
      }
      const bare = !e.metaKey && !e.altKey && !e.ctrlKey && !e.shiftKey;
      if (e.key === "Enter" && bare && combo) {
        commit(combo);
        return;
      }
      if (e.key === "Backspace" && bare) {
        setCombo(null);
        return;
      }
      const next = comboFromEvent(e);
      if (next) {
        setCombo(next);
        setHeld([]);
      } else {
        setHeld(
          displayKeys({
            code: "",
            meta: e.metaKey,
            ctrl: e.ctrlKey && !e.metaKey,
            shift: e.shiftKey,
            alt: e.altKey,
          }).slice(0, -1),
        );
      }
    };
    const onKeyUp = () => setHeld([]);
    window.addEventListener("keydown", onKeyDown, { capture: true });
    window.addEventListener("keyup", onKeyUp, { capture: true });
    return () => {
      window.removeEventListener("keydown", onKeyDown, { capture: true });
      window.removeEventListener("keyup", onKeyUp, { capture: true });
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [combo, locked]);

  const commit = (c: Combo) => {
    const s = serializeCombo(c);
    if (mode === "add") addBinding(actionId, s);
    else setBinding(actionId, [s]);
    onClose();
  };

  const same = combo ? bindingsForCombo(resolved.list, combo, actionId) : [];
  const hard = same.filter(
    (b) => b.when === def.when || b.when === "global" || def.when === "global",
  );

  return (
    <div
      className="absolute inset-0 z-20 flex items-start justify-center pt-[18%]"
      onMouseDown={(e) => {
        if (e.target === e.currentTarget) onClose();
      }}
    >
      <div
        className={cn(
          "w-[440px] rounded-lg border border-border bg-[var(--popover)]/95 backdrop-blur-xl",
          "shadow-md p-3 animate-in fade-in-0 duration-150",
        )}
      >
        {locked ? (
          <div className="space-y-2.5">
            <div className="flex items-center gap-2 text-sm font-medium text-foreground">
              <Lock size={12} className="text-muted-foreground" />
              The Default profile is locked
            </div>
            <p className="text-xs leading-relaxed text-secondary-foreground">
              Default always keeps Quotatlas's built-in shortcuts. Duplicate it into a new profile
              to change <span className="text-foreground">{def.title}</span> and anything else.
            </p>
            <div className="flex justify-end gap-2 pt-0.5">
              <button
                type="button"
                onClick={onClose}
                className="h-7 rounded-md px-2.5 text-xs font-medium text-secondary-foreground hover:text-foreground transition-colors"
              >
                Cancel
              </button>
              <button
                type="button"
                onClick={() => {
                  duplicateProfile(file.activeProfileId, "My keybindings");
                  // Stay open: the next render sees an editable profile and
                  // the recorder proper takes over.
                }}
                className="h-7 rounded-md px-2.5 text-xs font-medium bg-[var(--foreground)] text-[var(--background)] hover:opacity-90 transition-opacity"
              >
                Duplicate &amp; edit
              </button>
            </div>
          </div>
        ) : (
          <div className="space-y-2.5">
            <div className="text-center text-xs text-secondary-foreground">
              Press desired key combination and then press{" "}
              <span className="font-medium text-foreground">ENTER</span>.
              <div className="mt-0.5 text-2xs text-muted-foreground">
                {mode === "add" ? "Adding a keybinding to" : "Changing the keybinding for"}{" "}
                <span className="text-secondary-foreground">{def.title}</span>
              </div>
            </div>
            <div
              className={cn(
                "flex h-8 items-center justify-center rounded-md border bg-card px-2 font-mono text-sm",
                combo
                  ? "border-border-strong text-foreground"
                  : "border-border text-muted-foreground",
              )}
            >
              {combo
                ? serializeCombo(combo)
                : held.length
                  ? held.join(" ") + " …"
                  : "waiting for keys"}
            </div>
            <div className="flex h-5 items-center justify-center">
              {combo ? <KbdKeys keys={displayKeys(combo)} /> : null}
            </div>
            <div className="flex h-4 items-center justify-center text-xs">
              {combo && same.length > 0 ? (
                <button
                  type="button"
                  onClick={() => onShowSame(combo)}
                  className={cn(
                    "underline underline-offset-2 hover:opacity-80 transition-opacity cursor-pointer",
                    hard.length
                      ? "text-[var(--atlas-status-warning-foreground)]"
                      : "text-muted-foreground",
                  )}
                >
                  {same.length} existing {same.length === 1 ? "command has" : "commands have"} this
                  keybinding
                </button>
              ) : combo ? (
                <span className="text-muted-foreground">No other command uses this keybinding</span>
              ) : (
                <span className="text-muted-foreground">Esc to cancel · ⌫ to clear</span>
              )}
            </div>
          </div>
        )}
      </div>
    </div>
  );
}
