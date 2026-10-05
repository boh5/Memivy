import test from 'node:test';
import assert from 'node:assert/strict';
import { workspaceFixture } from './helpers/workspace.mjs';

function gestureFixture(t) {
  const f = workspaceFixture(t, { native: true });
  const errors = [], captured = new Set();
  f.overrides.desktop_drag = async () => {};
  f.overrides.desktop_open = async () => {};
  f.overrides.desktop_menu = async () => {};
  const { useDesktopGesture } = f.load('src/workspace/useDesktopGesture.ts');
  const component = ({ expanded, generation }) => useDesktopGesture(expanded, generation, error => errors.push(error));
  const view = f.mount(component, { expanded: false, generation: 7 });
  const element = {
    setPointerCapture(id) { captured.add(id); },
    hasPointerCapture(id) { return captured.has(id); },
    releasePointerCapture(id) { captured.delete(id); },
  };
  const pointer = (name, extra = {}) => view.tree.pointerHandlers[name]({
    button: 0, ctrlKey: false, isPrimary: true, pointerId: 1,
    screenX: 100, screenY: 200, clientX: 20, clientY: 20,
    currentTarget: element, target: { closest: () => element }, preventDefault() {}, ...extra,
  });
  const click = (extra = {}) => view.tree.onClick({ detail: 1, button: 0, ctrlKey: false, ...extra });
  const drags = () => f.calls.filter(call => call.name === 'desktop_drag')
    .map(({ args }) => [args.phase, args.gesture, args.x, args.y]);
  const opens = () => f.calls.filter(call => call.name === 'desktop_open').length;
  return { f, view, element, captured, errors, pointer, click, drags, opens };
}

test('a stationary press or small movement opens once without native dragging', async t => {
  const g = gestureFixture(t);
  g.pointer('onPointerDown', { timeStamp: 10 });
  g.pointer('onPointerUp', { screenX: 103, screenY: 204, timeStamp: 1500 });
  g.click();
  g.click();
  await g.f.settle();
  assert.equal(g.opens(), 1);
  assert.deepEqual(g.drags(), []);
  assert.equal(g.captured.size, 0);
});

test('release displacement prevents opening even when no move event arrives', async t => {
  const g = gestureFixture(t);
  g.pointer('onPointerDown');
  g.pointer('onPointerUp', { screenX: 137, screenY: 190 });
  g.click();
  await g.f.settle();
  const id = g.drags()[0][1];
  assert.deepEqual(g.drags(), [['start', id, 37, -10], ['end', id, 37, -10]]);
  assert.equal(g.opens(), 0);
  g.pointer('onPointerDown', { screenX: 300 });
  g.pointer('onPointerUp', { screenX: 300 });
  g.click();
  await g.f.settle();
  assert.equal(g.opens(), 1);
});

test('dragging follows screen coordinates while the moving webview keeps client coordinates unchanged', async t => {
  const g = gestureFixture(t);
  g.pointer('onPointerDown');
  g.pointer('onPointerMove', { screenX: 110, screenY: 205 });
  g.pointer('onPointerMove', { screenX: 140, screenY: 207 });
  g.pointer('onPointerUp', { screenX: 148, screenY: 209 });
  g.click();
  await g.f.settle();
  const id = g.drags()[0][1];
  assert.deepEqual(g.drags(), [['start', id, 10, 5], ['move', id, 40, 7], ['end', id, 48, 9]]);
  assert.equal(g.opens(), 0);
});

test('a drag that returns to its origin does not open and leaves accessible activation usable', async t => {
  const g = gestureFixture(t);
  g.pointer('onPointerDown');
  g.pointer('onPointerMove', { screenX: 140 });
  g.pointer('onPointerUp');
  await g.f.settle();
  assert.equal(g.opens(), 0);
  // The platform may omit the pointer click after a drag; activation must not depend on consuming it.
  g.click({ detail: 0 });
  await g.f.settle();
  assert.equal(g.opens(), 1);
  assert.deepEqual(g.drags().map(([phase, , x, y]) => [phase, x, y]), [['start', 40, 0], ['end', 0, 0]]);
});

test('unrelated pointers cannot move, release or cancel the captured gesture', async t => {
  const g = gestureFixture(t);
  g.pointer('onPointerDown');
  g.pointer('onPointerMove', { pointerId: 2, screenX: 500 });
  g.pointer('onPointerUp', { pointerId: 2, screenX: 500 });
  g.pointer('onPointerCancel', { pointerId: 2 });
  g.pointer('onLostPointerCapture', { pointerId: 2 });
  assert.equal(g.captured.has(1), true);
  g.pointer('onPointerUp');
  g.click();
  await g.f.settle();
  assert.equal(g.opens(), 1);
  assert.deepEqual(g.drags(), []);
});

test('cancellation and lost capture retain the last offset and cannot activate the leaf', async t => {
  for (const cancellation of ['onPointerCancel', 'onLostPointerCapture']) {
    const g = gestureFixture(t);
    g.pointer('onPointerDown');
    g.pointer('onPointerMove', { screenX: 130, screenY: 215 });
    g.pointer(cancellation, { screenX: 0, screenY: 0 });
    g.pointer('onPointerUp');
    g.click();
    await g.f.settle();
    const id = g.drags()[0][1];
    assert.deepEqual(g.drags(), [['start', id, 30, 15], ['cancel', id, 30, 15]]);
    assert.equal(g.opens(), 0);
    assert.equal(g.captured.size, 0);
    g.click({ detail: 0 });
    await g.f.settle();
    assert.equal(g.opens(), 1);
  }
});

test('secondary and control clicks open only the native context menu', async t => {
  for (const extra of [{ button: 2 }, { ctrlKey: true }]) {
    const g = gestureFixture(t);
    g.pointer('onPointerDown', extra);
    g.pointer('onPointerUp', extra);
    let prevented = false;
    const context = { ...extra, clientX: 24, clientY: 35, preventDefault() { prevented = true; } };
    g.view.tree.onContextMenu(context);
    context.clientX = 900;
    context.clientY = 700;
    g.click(extra);
    await g.f.settle();
    assert.equal(prevented, true);
    assert.equal(g.opens(), 0);
    assert.deepEqual(g.drags(), []);
    assert.deepEqual(g.f.calls.filter(call => call.name === 'desktop_menu').map(call => [call.args.x, call.args.y]), [[24, 35]]);
  }
});

test('a delayed native drag finishes before the next gesture and uses the final release offsets', async t => {
  const g = gestureFixture(t);
  let completeFirstStart;
  g.f.overrides.desktop_drag = args => args.phase === 'start' && !completeFirstStart
    ? new Promise(resolve => { completeFirstStart = resolve; }) : Promise.resolve();
  g.pointer('onPointerDown');
  g.pointer('onPointerMove', { screenX: 110 });
  await g.f.settle();
  g.pointer('onPointerMove', { screenX: 120 });
  g.pointer('onPointerUp', { screenX: 130 });
  g.pointer('onPointerDown', { screenX: 400 });
  g.pointer('onPointerMove', { screenX: 420 });
  g.pointer('onPointerUp', { screenX: 435 });
  await g.f.settle();
  assert.equal(g.drags().length, 1);
  completeFirstStart();
  await g.f.settle();
  const first = g.drags()[0][1], second = g.drags()[3][1];
  assert.notEqual(first, second);
  assert.deepEqual(g.drags(), [
    ['start', first, 10, 0], ['move', first, 20, 0], ['end', first, 30, 0],
    ['start', second, 20, 0], ['end', second, 35, 0],
  ]);
  assert.equal(g.opens(), 0);
  assert.deepEqual(g.errors, []);
});

test('window-mode changes and unmount cancel dragging without activating the leaf', async t => {
  for (const transition of ['mode', 'unmount']) {
    const g = gestureFixture(t);
    g.pointer('onPointerDown');
    g.pointer('onPointerMove', { screenX: 130 });
    if (transition === 'mode') g.f.render(g.view, { expanded: true, generation: 8 });
    else g.f.unmount(g.view);
    await g.f.settle();
    const id = g.drags()[0][1];
    assert.deepEqual(g.drags(), [['start', id, 30, 0], ['cancel', id, 30, 0]]);
    assert.equal(g.captured.size, 0);
    assert.equal(g.opens(), 0);
    if (transition === 'mode') {
      g.pointer('onPointerUp');
      g.click();
      await g.f.settle();
      assert.equal(g.opens(), 0);
    }
  }
});


test('a new window generation cancels the old gesture while queued commands keep their original generation', async t => {
  const g = gestureFixture(t);
  g.f.render(g.view, { expanded: true, generation: 7 });
  let completeFirstStart;
  g.f.overrides.desktop_drag = args => args.phase === 'start' && !completeFirstStart
    ? new Promise(resolve => { completeFirstStart = resolve; }) : Promise.resolve();
  g.pointer('onPointerDown');
  g.pointer('onPointerMove', { screenX: 110 });
  await g.f.settle();
  g.pointer('onPointerMove', { screenX: 120 });
  // Reopening an already expanded window can change its generation without changing its mode.
  g.f.render(g.view, { expanded: true, generation: 8 });
  assert.equal(g.captured.size, 0);
  g.pointer('onPointerUp', { screenX: 150 });
  g.click();
  g.pointer('onPointerDown', { screenX: 400 });
  g.pointer('onPointerMove', { screenX: 420 });
  g.pointer('onPointerUp', { screenX: 425 });
  await g.f.settle();
  assert.equal(g.drags().length, 1);
  completeFirstStart();
  await g.f.settle();
  assert.deepEqual(g.f.calls.filter(call => call.name === 'desktop_drag').map(({ args }) =>
    [args.phase, args.generation, args.x, args.y]), [
    ['start', 7, 10, 0], ['move', 7, 20, 0], ['cancel', 7, 20, 0],
    ['start', 8, 20, 0], ['end', 8, 25, 0],
  ]);
  assert.equal(g.opens(), 0);
  assert.deepEqual(g.errors, []);
});
