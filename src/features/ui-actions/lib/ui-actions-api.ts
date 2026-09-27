/** The IPC surface of UI actions: the one event and the one command. */

import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type { UiActionReply, UiActionRequest } from "./types";

export function listenUiAction(handler: (request: UiActionRequest) => void): Promise<UnlistenFn> {
  return listen<UiActionRequest>("atlas:ui-action", (event) => handler(event.payload));
}

export function respondUiAction(requestId: string, reply: UiActionReply): Promise<void> {
  return invoke("ui_action_respond", { requestId, reply });
}
