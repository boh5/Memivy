import test from 'node:test';
import assert from 'node:assert/strict';
import { installClickRecovery, ReorderedPointer } from '../src/workspace/clickRecovery.ts';

// Small DOM boundary double: exercise the installed listeners and real timers,
// including late native clicks, without adding a browser runtime dependency.
function fixture(t) {
  const listeners = new Map();
  const winListeners = new Map();
  let root;
  class Element {
    constructor(kind = 'button') {
      this.kind = kind; this.isConnected = true; this.disabled = false;
      this.visible = true; this.activations = 0;
    }
    closest(selector) {
      if (selector === '[inert]') return null;
      return this.owner || (['button', 'summary', 'a'].includes(this.kind) ? this : null);
    }
    matches(selector) {
      return selector.includes('textarea') ? this.kind === 'textarea' : this.disabled;
    }
    getClientRects() { return this.visible ? [{}] : []; }
    click() { root.fire('click', this, { isTrusted: false, detail: 0 }); }
  }
  root = {
    addEventListener(type, fn) { listeners.set(type, fn); },
    removeEventListener(type) { listeners.delete(type); },
    contains(element) { return element.inRoot !== false; },
    fire(type, target, extra = {}) {
      const event = {
        target, isTrusted: true, pointerType: 'mouse', isPrimary: true,
        pointerId: 1, clientX: 10, clientY: 10, button: 0, detail: 1,
        altKey: false, ctrlKey: false, metaKey: false, shiftKey: false,
        preventDefault() { this.prevented = true; },
        stopImmediatePropagation() { this.stopped = true; },
        ...extra,
      };
      listeners.get(type)?.(event);
      if (type === 'click' && !event.stopped && !target.disabled)
        (target.owner || target).activations++;
      return event;
    },
  };
  const previous = new Map(['Element', 'document', 'window'].map(k => [k, globalThis[k]]));
  globalThis.Element = Element;
  globalThis.document = { activeElement: new Element('textarea') };
  globalThis.window = {
    addEventListener(type, fn) { winListeners.set(type, fn); },
    removeEventListener(type) { winListeners.delete(type); },
  };
  const cleanup = installClickRecovery(root);
  t.after(() => { cleanup(); for (const [k, v] of previous) v === undefined ? delete globalThis[k] : globalThis[k] = v; });
  const button = new Element();
  return { button, root, Element, blur: () => winListeners.get('blur')?.() };
}
const settle = () => new Promise(resolve => setTimeout(resolve, 5));
const reversed = ({root, button}) => {
  root.fire('pointerup', button);
  root.fire('pointerdown', button);
};

test('ordinary pointer click invokes its action once', async t => {
  const f = fixture(t);
  f.root.fire('pointerdown', f.button);
  f.root.fire('pointerup', f.button);
  f.root.fire('click', f.button);
  await settle();
  assert.equal(f.button.activations, 1);
});
test('reordered release and press recover one completed click', async t => {
  const f = fixture(t); reversed(f); await settle();
  assert.equal(f.button.activations, 1);
});
test('a real click arriving before recovery cancels the synthetic click', async t => {
  const f = fixture(t); reversed(f); f.root.fire('click', f.button); await settle();
  assert.equal(f.button.activations, 1);
});
test('a late real click is suppressed, but the next intentional click works', async t => {
  const f = fixture(t); reversed(f); await settle();
  assert.equal(f.root.fire('click', f.button).prevented, true);
  assert.equal(f.button.activations, 1);
  f.root.fire('pointerdown', f.button);
  f.root.fire('pointerup', f.button);
  f.root.fire('click', f.button);
  await settle(); assert.equal(f.button.activations, 2);
});
test('keyboard activation cancels pending recovery and stays usable', async t => {
  const f = fixture(t); reversed(f);
  f.root.fire('click', f.button, { detail: 0 }); await settle();
  assert.equal(f.button.activations, 1);
});
test('dragging away from a button never activates either target', async t => {
  const f = fixture(t); const other = new f.Element();
  f.root.fire('pointerdown', f.button);
  f.root.fire('pointerup', other, { clientX: 200 });
  f.root.fire('pointerdown', other, { clientX: 200 });
  await settle();
  assert.equal(f.button.activations + other.activations, 0);
});
test('removed, disabled and hidden controls are rechecked before dispatch', async t => {
  const f = fixture(t);
  for (const property of ['isConnected', 'visible', 'disabled']) {
    f.button.isConnected = true; f.button.visible = true; f.button.disabled = false;
    reversed(f); f.button[property] = property === 'disabled'; await settle();
  }
  assert.equal(f.button.activations, 0);
});
test('window deactivation and pointer cancellation clear pending gestures', async t => {
  const f = fixture(t); reversed(f); f.blur(); await settle();
  reversed(f); f.root.fire('pointercancel', f.button); await settle();
  assert.equal(f.button.activations, 0);
});
test('secondary buttons, modifiers, touch and untrusted pointers are untouched', async t => {
  const f = fixture(t);
  for (const options of [{button: 2}, {metaKey: true}, {shiftKey: true}, {pointerType: 'touch'}, {isTrusted: false}]) {
    f.root.fire('pointerup', f.button, options);
    f.root.fire('pointerdown', f.button, options); await settle();
  }
  assert.equal(f.button.activations, 0);
});
test('recovery requires an editor to have focus at release', async t => {
  const f = fixture(t); document.activeElement = new f.Element('div');
  reversed(f); await settle(); assert.equal(f.button.activations, 0);
});
test('icon and label targets within the same control recover once', async t => {
  const f = fixture(t); const icon = new f.Element('svg'); icon.owner = f.button;
  f.root.fire('pointerup', icon); f.root.fire('pointerdown', f.button); await settle();
  assert.equal(f.button.activations, 1);
});
test('stale, spatially different and different-pointer releases are rejected', () => {
  const a = {}, b = {};
  const base = {id: 1, action: a, x: 10, y: 10, at: 10, editing: true};
  for (const change of [{at: 261}, {x: 20}, {action: b}, {id: 2}]) {
    const sequence = new ReorderedPointer();
    sequence.up(base);
    assert.equal(sequence.down({...base, at: 12, ...change}), false);
  }
});
