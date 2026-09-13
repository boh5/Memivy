import { message, type UiMessage } from "../i18n/messages";
import { useNotice } from "../i18n/react";
import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { call, errorText, native, type Key, type Topic } from "./api";
import { finishVoiceInputs } from "./useVoice";
import { flushDrafts, refreshDrafts } from "./useDraft";

export type DesktopState = {
  expanded: boolean; generation: number; sequence?: number; pinned: boolean; visible: boolean;
  paused: boolean; shortcut: string; topic: Topic | null;
  source_app: string; last_memory: string | null; error: string | null;
  configured: boolean; ready_ms: number | null; save_ms: number | null;
  receipt: boolean;
};
export type DesktopPatch = Partial<Pick<DesktopState, "visible" | "paused" | "shortcut" | "pinned">> & { topic_id?: string; clear_topic?: boolean };
export type MainRoute = { generation: number; topic: Topic | null; quick: boolean; record: Key | null; settings: boolean };
export const previewDesktop: DesktopState = { expanded: true, generation: 1, pinned: false, visible: true,
  paused: false, shortcut: "Control+Super+KeyM", topic: null, source_app: "Safari",
  last_memory: null, error: null, configured: false, ready_ms: null, save_ms: null, receipt: false };
export function useDesktop() {
  const [state, setState] = useState<DesktopState | null>(native ? null : previewDesktop);
  const [error, setError] = useNotice();
  const newest = useRef(-1), requests = useRef(0);
  const accept = useCallback((next: DesktopState) => {
    if (!next) { setError(message("errors", "desktop_status_failed")); return; }
    const sequence = next.sequence ?? next.generation;
    if (sequence < newest.current) return;
    newest.current = sequence;
    setState(previous => previous && JSON.stringify({ ...previous, sequence: next.sequence }) === JSON.stringify(next) ? previous : next);
    setError("");
  }, []);
  const refresh = useCallback(async () => {
    const request = ++requests.current;
    if (native) try { const next = await call<DesktopState>("desktop_state"); if (request === requests.current) accept(next); }
    catch (e) { if (request === requests.current) setError(errorText(e)); }
  }, [accept]);
  useEffect(() => {
    if (!native) return;
    const events = [listen<DesktopState>("desktop-state", e => { requests.current++; accept(e.payload); }), listen("settings-changed", () => void refresh())];
    void Promise.all(events).then(() => refresh());
    return () => { requests.current++; events.forEach(x => void x.then(stop => stop())); };
  }, [refresh, accept]);
  async function update(patch: DesktopPatch) {
    if (!native) { setState(s => s && { ...s, ...patch, ...(patch.clear_topic ? { topic: null } : {}) }); return; }
    accept(await call<DesktopState>("desktop_update", { patch }));
  }
  return { state, error, refresh, update };
}
// Both WebViews acknowledge exit only after their own draft queue is durable.
export function useWindowLifecycle(onError: (error: UiMessage) => void) {
  const errorRef = useRef(onError); errorRef.current = onError;
  useEffect(() => {
    if (!native) return;
    const events = [
      listen<string>("draft-changed", e => void refreshDrafts(e.payload).catch(e => errorRef.current(errorText(e)))),
      listen<number>("desktop-exit-request", e => {
        // An unconfirmed conclusion or settings form must remain reviewable.
        if (document.querySelector("dialog[open]")) {
          errorRef.current(message("errors", "quit_dialog_active"));
          void call("desktop_exit_ready", { id: e.payload, error: true });
          return;
        }
        void finishVoiceInputs().then(() => flushDrafts()).then(() => call("desktop_exit_ready", { id: e.payload, error: false }))
          .catch(error => { errorRef.current(message("errors", "window_draft_preserved", { error: errorText(error) })); void call("desktop_exit_ready", { id: e.payload, error: true }); });
      }),
    ];
    void Promise.all(events).then(() => call("desktop_ready", { generation: null })).catch(e => errorRef.current(errorText(e)));
    return () => { events.forEach(x => void x.then(stop => stop())); };
  }, []);
}
export function shortcutLabel(shortcut: string) {
  return shortcut.split("+").map(s => ({ Control: "⌃", Alt: "⌥", Shift: "⇧", Super: "⌘" })[s] || s.replace(/^Key|^Digit/, "")).join("");
}
