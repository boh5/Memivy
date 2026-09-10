import { useCallback, useEffect, useRef, useState } from "react";
import { listen } from "@tauri-apps/api/event";
import { call, errorText, native, type Key, type Topic } from "./api";
import { flushDrafts, refreshDrafts } from "./useDraft";

export type DesktopState = {
  expanded: boolean; generation: number; pinned: boolean; visible: boolean;
  paused: boolean; shortcut: string; mode: "capture" | "ask"; topic: Topic | null;
  source_app: string; last_memory: string | null; error: string | null;
  configured: boolean; ready_ms: number | null; save_ms: number | null;
  receipt: boolean;
};
export type DesktopPatch = Partial<Pick<DesktopState, "visible" | "paused" | "shortcut" | "pinned" | "mode">> & { topic_id?: string; clear_topic?: boolean };
export type MainRoute = { generation: number; topic: Topic | null; mode: "capture" | "ask"; quick: boolean; record: Key | null; settings: boolean };
export const previewDesktop: DesktopState = { expanded: true, generation: 1, pinned: false, visible: true,
  paused: false, shortcut: "Control+Super+KeyM", mode: "capture", topic: null, source_app: "Safari",
  last_memory: null, error: null, configured: false, ready_ms: null, save_ms: null, receipt: false };
export function useDesktop() {
  const [state, setState] = useState<DesktopState | null>(native ? null : previewDesktop);
  const [error, setError] = useState("");
  const refresh = useCallback(async () => {
    if (native) try { setState(await call<DesktopState>("desktop_state")); } catch (e) { setError(errorText(e)); }
  }, []);
  useEffect(() => {
    if (!native) return;
    const events = [listen<DesktopState>("desktop-state", e => setState(e.payload)), listen("library-refresh", () => void refresh())];
    void refresh();
    return () => { events.forEach(x => void x.then(stop => stop())); };
  }, [refresh]);
  async function update(patch: DesktopPatch) {
    if (!native) { setState(s => s && { ...s, ...patch, ...(patch.clear_topic ? { topic: null } : {}) }); return; }
    setState(await call<DesktopState>("desktop_update", { patch }));
  }
  return { state, error, refresh, update };
}
// Both WebViews acknowledge exit only after their own draft queue is durable.
export function useWindowLifecycle(onError: (error: string) => void) {
  const errorRef = useRef(onError); errorRef.current = onError;
  useEffect(() => {
    if (!native) return;
    const events = [
      listen("draft-changed", () => void refreshDrafts().catch(e => errorRef.current(errorText(e)))),
      listen<number>("desktop-exit-request", e => {
        // An unconfirmed conclusion or settings form must remain reviewable.
        if (document.querySelector("dialog[open]")) {
          errorRef.current("请先完成或关闭当前对话框，再退出 Memivy。");
          void call("desktop_exit_ready", { id: e.payload, error: true });
          return;
        }
        void flushDrafts().then(() => call("desktop_exit_ready", { id: e.payload, error: false }))
          .catch(error => { errorRef.current(`草稿尚未保存，已保留窗口。${errorText(error)}`); void call("desktop_exit_ready", { id: e.payload, error: true }); });
      }),
    ];
    void Promise.all(events).then(() => call("desktop_ready", { generation: null })).catch(e => errorRef.current(errorText(e)));
    return () => { events.forEach(x => void x.then(stop => stop())); };
  }, []);
}
export function shortcutLabel(shortcut: string) {
  return shortcut.split("+").map(s => ({ Control: "⌃", Alt: "⌥", Shift: "⇧", Super: "⌘" })[s] || s.replace(/^Key|^Digit/, "")).join("");
}
