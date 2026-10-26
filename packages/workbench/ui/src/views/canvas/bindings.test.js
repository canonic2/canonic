import test from 'node:test';
import assert from 'node:assert/strict';
import { bindCanvas } from './bindings.js';

test('canvas bindings mount with supplied targets and actions and release their lifetime', (t) => {
  const observed = [],
    disconnected = [];
  const before = Object.getOwnPropertyDescriptor(globalThis, 'ResizeObserver');
  Object.defineProperty(globalThis, 'ResizeObserver', {
    configurable: true,
    value: class {
      constructor(run) {
        this.run = run;
      }
      observe(target) {
        observed.push(target);
        this.run();
      }
      disconnect() {
        disconnected.push(this);
      }
    },
  });
  t.after(() => {
    if (before) Object.defineProperty(globalThis, 'ResizeObserver', before);
    else delete globalThis.ResizeObserver;
  });
  const targets = {
    canvas: new EventTarget(),
    room: {},
    shoot: new EventTarget(),
    handoff: new EventTarget(),
  };
  const calls = [];
  const actions = Object.fromEntries(
    [
      'select',
      'resize',
      'deselect',
      'focus',
      'readingResize',
      'readingScroll',
      'refill',
      'shoot',
      'handoff',
    ].map((name) => [name, (value) => calls.push([name, value])]),
  );
  const lifetime = new AbortController();
  bindCanvas(targets, actions, { signal: lifetime.signal });
  targets.canvas.dispatchEvent(new CustomEvent('wb-artboard-select', { detail: { id: 'one' } }));
  targets.canvas.dispatchEvent(
    new CustomEvent('wb-artboard-resize', { detail: { id: 'one', width: 320, height: 180 } }),
  );
  targets.shoot.dispatchEvent(new Event('click'));
  assert.deepEqual(observed, [targets.room]);
  assert.deepEqual(calls.slice(0, 3), [
    ['refill', undefined],
    ['select', 'one'],
    ['resize', { id: 'one', width: 320, height: 180 }],
  ]);
  assert.equal(calls.at(-1)[0], 'shoot');
  lifetime.abort();
  const count = calls.length;
  targets.canvas.dispatchEvent(new CustomEvent('wb-artboard-select', { detail: { id: 'two' } }));
  targets.shoot.dispatchEvent(new Event('click'));
  bindCanvas(targets, actions, { signal: lifetime.signal });
  assert.equal(calls.length, count);
  assert.equal(observed.length, 1);
  assert.equal(disconnected.length, 1);
});
