import { isWindows } from "@/lib/platform";

/**
 * The drag position in CSS px. Tauri types it `PhysicalPosition`, but the unit
 * is whatever wry's platform backend produced (wry 0.55):
 *  - macOS: `draggingLocation`, in points — already CSS px.
 *  - Linux: GTK `drag-motion` x/y, in logical px — already CSS px.
 *  - Windows: `ScreenToClient`, in device pixels — divide by the scale.
 * Don't "try raw, then scaled": on a scaled Windows display the raw point
 * usually lands on some other real element first, and the wrong zone lights up.
 */
export function dragPositionToCss(position: { x: number; y: number }): { x: number; y: number } {
  if (!isWindows) return { x: position.x, y: position.y };
  const dpr = window.devicePixelRatio || 1;
  return { x: position.x / dpr, y: position.y / dpr };
}
