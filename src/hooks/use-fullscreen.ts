import { useEffect, useState } from "react";

/**
 * True while the native window is in macOS fullscreen. In fullscreen the
 * traffic-light controls are hidden, so chrome that normally dodges them — the
 * titlebar's 72px left inset, the project sidebar's top-bar buttons — can
 * reclaim the left edge.
 *
 * # Why the sequence guard
 *
 * There is no fullscreen event in Tauri v2, so the only signal is `onResized`,
 * and the answer has to be fetched with `await win.isFullscreen()` — an IPC
 * round trip per event.
 *
 * macOS animates the fullscreen transition, which fires a **burst** of resize
 * events, and those round trips can resolve out of order. An early query
 * (answering `true`, mid-exit) resolving *after* the final one (answering
 * `false`) latches the wrong value with nothing left to correct it: the resize
 * stream has ended, so no further event arrives.
 *
 * That is not cosmetic. A stuck `true` makes the titlebar drop its traffic-light
 * inset while the lights are still there — the window controls end up underneath
 * the Spaces and panel buttons — and pushes the sidebar's pin/collapse buttons
 * left underneath them too, so they read as missing.
 *
 * The counter fixes the ordering: a resolution from anything but the newest
 * query is dropped. The focus re-check is the backstop for a transition that
 * completed while this window was not frontmost, where the burst can be
 * coalesced away entirely.
 */
export function useFullscreen(): boolean {
  const [fullscreen, setFullscreen] = useState(false);

  useEffect(() => {
    let disposed = false;
    const stops: Array<() => void> = [];
    // Monotonic: only the newest query may write.
    let seq = 0;

    (async () => {
      try {
        const { getCurrentWindow } = await import("@tauri-apps/api/window");
        const win = getCurrentWindow();

        const read = async () => {
          const mine = ++seq;
          const next = await win.isFullscreen();
          // A stale answer from an earlier query, or the component is gone.
          if (disposed || mine !== seq) return;
          setFullscreen(next);
        };

        await read();
        if (disposed) return;

        stops.push(await win.onResized(() => void read()));
        // Regaining focus is when a missed transition becomes visible, and it
        // is cheap enough to re-check unconditionally.
        stops.push(await win.onFocusChanged(() => void read()));
      } catch {
        // not in a Tauri context
      }
    })();

    return () => {
      disposed = true;
      for (const stop of stops) stop();
    };
  }, []);

  return fullscreen;
}
