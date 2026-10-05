import { useEffect, useRef, type MouseEvent, type PointerEvent } from "react";
import { call, native } from "./api";

const DRAG_DISTANCE = 5;
let nextGesture = Date.now();
// One queue also covers a gesture cancelled by a component remount.
let commands: Promise<unknown> = Promise.resolve();

type Gesture = {
  id: number;
  generation: number;
  pointer: number;
  element: HTMLElement;
  startX: number;
  startY: number;
  x: number;
  y: number;
  moved: boolean;
};

export function useDesktopGesture(expanded: boolean, generation: number, onError: (error: unknown) => void) {
  const active = useRef<Gesture | null>(null);
  const pointerClick = useRef(false);
  const report = useRef(onError);
  report.current = onError;

  function enqueue(action: () => Promise<unknown>) {
    commands = commands.then(action).catch(error => report.current(error));
  }
  function send(phase: "start" | "move" | "end" | "cancel", gesture: Gesture) {
    if (!native) return;
    const args = { phase, gesture: gesture.id, generation: gesture.generation, x: gesture.x, y: gesture.y };
    enqueue(() => call("desktop_drag", args));
  }
  function release(gesture: Gesture) {
    active.current = null;
    if (gesture.element.hasPointerCapture(gesture.pointer)) {
      gesture.element.releasePointerCapture(gesture.pointer);
    }
  }
  function cancel() {
    pointerClick.current = false;
    const gesture = active.current;
    if (!gesture) return;
    release(gesture);
    if (gesture.moved) send("cancel", gesture);
  }
  function update(gesture: Gesture, event: PointerEvent<HTMLElement>) {
    // Client coordinates change when the panel itself moves under the pointer.
    gesture.x = event.screenX - gesture.startX;
    gesture.y = event.screenY - gesture.startY;
    if (!gesture.moved && Math.hypot(gesture.x, gesture.y) > DRAG_DISTANCE) {
      gesture.moved = true;
      send("start", gesture);
      return true;
    }
    return false;
  }
  useEffect(() => {
    window.addEventListener("blur", cancel);
    return () => { window.removeEventListener("blur", cancel); cancel(); };
  }, [expanded, generation]);

  const pointerHandlers = {
    onPointerDown(event: PointerEvent<HTMLElement>) {
      pointerClick.current = false;
      const control = (event.target as HTMLElement).closest("button, input, textarea, a, summary");
      if (active.current || event.button !== 0 || event.ctrlKey || !event.isPrimary ||
          (control && control !== event.currentTarget)) return;
      // Keep the editor's focus and selection while moving its window.
      event.preventDefault();
      event.currentTarget.setPointerCapture(event.pointerId);
      active.current = {
        id: ++nextGesture, generation, pointer: event.pointerId, element: event.currentTarget,
        startX: event.screenX, startY: event.screenY, x: 0, y: 0, moved: false,
      };
    },
    onPointerMove(event: PointerEvent<HTMLElement>) {
      const gesture = active.current;
      if (!gesture || gesture.pointer !== event.pointerId) return;
      const started = update(gesture, event);
      if (gesture.moved && !started) send("move", gesture);
    },
    onPointerUp(event: PointerEvent<HTMLElement>) {
      const gesture = active.current;
      if (!gesture || gesture.pointer !== event.pointerId) return;
      update(gesture, event);
      release(gesture);
      pointerClick.current = !gesture.moved;
      if (gesture.moved) send("end", gesture);
    },
    onPointerCancel(event: PointerEvent<HTMLElement>) {
      if (active.current?.pointer === event.pointerId) cancel();
    },
    onLostPointerCapture(event: PointerEvent<HTMLElement>) {
      if (active.current?.pointer === event.pointerId) cancel();
    },
  };
  return {
    pointerHandlers,
    onClick(event: MouseEvent<HTMLElement>) {
      const activate = event.detail === 0 || (event.button === 0 && !event.ctrlKey && pointerClick.current);
      pointerClick.current = false;
      if (activate) enqueue(() => call("desktop_open"));
    },
    onContextMenu(event: MouseEvent<HTMLElement>) {
      if (!native) return;
      event.preventDefault();
      cancel();
      const { clientX: x, clientY: y } = event;
      enqueue(() => call("desktop_menu", { x, y }));
    },
  };
}
