// WebKit can deliver mouse-up before mouse-down when leaving an IME editor.
// Recognize that completed gesture; ordinary clicks and drags keep native behavior.
// https://bugs.webkit.org/show_bug.cgi?id=219670
export type PointerSample = {
  id: number;
  action: object | null;
  x: number;
  y: number;
  at: number;
  editing: boolean;
};

export class ReorderedPointer {
  private pressed: number | null = null;
  private released: PointerSample | null = null;

  down(sample: PointerSample): boolean {
    const up = this.released;
    this.released = null;
    const reversed = !!(
      up && up.id === sample.id && up.action && up.action === sample.action &&
      sample.at >= up.at && sample.at - up.at <= 250 &&
      Math.hypot(sample.x - up.x, sample.y - up.y) <= 4
    );
    this.pressed = reversed ? null : sample.id;
    return reversed;
  }

  up(sample: PointerSample) {
    this.released = this.pressed === sample.id
      ? null
      : sample.editing && sample.action ? sample : null;
    this.pressed = null;
  }

  reset() {
    this.pressed = null;
    this.released = null;
  }
}

export function installClickRecovery(root: HTMLElement) {
  const sequence = new ReorderedPointer();
  let pending: { action: HTMLElement; fired: boolean } | null = null;
  let timer: ReturnType<typeof setTimeout> | undefined;
  const actionFor = (target: EventTarget | null) => {
    const action = target instanceof Element
      ? target.closest<HTMLElement>("button, summary, a[href]") : null;
    return action && root.contains(action) ? action : null;
  };
  const enabled = (action: HTMLElement) =>
    action.isConnected && !action.matches(":disabled, [aria-disabled='true']") &&
    !action.closest("[inert]") && action.getClientRects().length > 0;
  const clearPending = () => {
    clearTimeout(timer);
    pending = null;
  };
  const reset = () => {
    sequence.reset();
    clearPending();
  };
  const sample = (event: PointerEvent): PointerSample => ({
    id: event.pointerId,
    action: actionFor(event.target),
    x: event.clientX,
    y: event.clientY,
    at: performance.now(),
    editing: !!document.activeElement?.matches("input, textarea, [contenteditable='true']"),
  });
  const primaryMouse = (event: PointerEvent) =>
    event.isTrusted && event.pointerType === "mouse" && event.isPrimary &&
    event.button === 0 && !event.altKey && !event.ctrlKey && !event.metaKey && !event.shiftKey;
  const down = (event: PointerEvent) => {
    // A new physical gesture must never be suppressed as a duplicate.
    clearPending();
    if (!primaryMouse(event)) { sequence.reset(); return; }
    const action = actionFor(event.target);
    if (!sequence.down(sample(event)) || !action || !enabled(action)) return;
    const activation = { action, fired: false };
    pending = activation;
    // Allow WebKit's delayed focus/default action and any real click to finish.
    timer = setTimeout(() => {
      if (pending !== activation || !enabled(action)) return;
      activation.fired = true;
      action.click();
    }, 0);
  };
  const up = (event: PointerEvent) => {
    if (primaryMouse(event)) sequence.up(sample(event));
    else sequence.reset();
  };
  const click = (event: MouseEvent) => {
    if (!event.isTrusted || !pending) return;
    if (event.detail === 0 || actionFor(event.target) !== pending.action) {
      clearPending();
      return;
    }
    if (pending.fired) {
      // Some WebKit versions may still send a late click for this same gesture.
      event.preventDefault();
      event.stopImmediatePropagation();
    }
    clearPending();
  };
  root.addEventListener("pointerdown", down, true);
  root.addEventListener("pointerup", up, true);
  root.addEventListener("pointercancel", reset, true);
  root.addEventListener("click", click, true);
  window.addEventListener("blur", reset);
  return () => {
    reset();
    root.removeEventListener("pointerdown", down, true);
    root.removeEventListener("pointerup", up, true);
    root.removeEventListener("pointercancel", reset, true);
    root.removeEventListener("click", click, true);
    window.removeEventListener("blur", reset);
  };
}
