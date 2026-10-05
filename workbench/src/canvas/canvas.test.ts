import test from 'node:test';
import assert from 'node:assert/strict';
import { add, change, createState, remove, size, supportsSize } from './model.ts';
import type { Target, ViewInfo } from './model.ts';
import { fit, zoom } from './geometry.ts';
import { createController } from './controller.ts';
import { reference, report } from './review.ts';
import { cleanView } from '../agent-context/report.ts';
import type { Runtime } from './runtime.ts';

const target: Target = { space: { id: 'acme', name: 'Acme', url: 'http://127.0.0.1:3579/_workbench/' }, src: 'sign-in.html', state: null, lens: null };
const ready: ViewInfo = { src: target.src, state: null, lens: null, label: 'Sign in', reference: 'Page: sign-in.html', hash: '#sign-in.html@resizable', ready: true, problem: null, payload: {}, states: [], lenses: [] };
const dimensions = { width: 1512, height: 982 };

test('duplicate descriptors retain independent sizes and insertion-order geometry; closing selects a neighbour', () => {
  const first = add(createState('canvas'), 'a', target, dimensions);
  const second = add(first, 'b', target, { width: 393, height: 852 });
  assert.equal(second.artboards[1]!.x, 1560);
  const resized = change(second, 'a', { size: size(1024, 768) });
  assert.equal(resized.artboards[1]!.x, 1072);
  assert.equal(first.artboards[0]!.size.width, 1512, 'prior snapshots stay immutable');
  assert.equal(resized.artboards[1]!.size.width, 393);
  assert.equal(remove(resized, 'b').selected, 'a');
  assert.equal(remove(remove(resized, 'b'), 'a').selected, null);
  assert.throws(() => add(first, 'a', target, dimensions), /unique/);
  assert.throws(() => size(NaN, 768), /dimensions/);
});

test('fitting a selected board centers its absolute position; zoom preserves the point under the pointer', () => {
  const state = add(add(createState('canvas'), 'a', target, dimensions), 'b', target, { width: 393, height: 852 });
  const camera = fit([state.artboards[1]!], { width: 1000, height: 1000 });
  assert.equal(camera.x + state.artboards[1]!.x * camera.scale, (1000 - 393 * camera.scale) / 2);
  const pointer = { x: 400, y: 500 }, next = zoom(camera, 2, pointer);
  assert.equal((pointer.x - camera.x) / camera.scale, (pointer.x - next.x) / next.scale);
});

test('fit all includes the largest supported row rather than stopping at a single-artboard zoom floor', () => {
  let state = createState('canvas');
  for (let i = 0; i < 32; i++) state = add(state, String(i), target, { width: 8192, height: 8192 });
  const camera = fit(state.artboards, { width: 800, height: 600 });
  const last = state.artboards.at(-1)!;
  assert.ok(camera.x + (last.x + last.size.width) * camera.scale <= 800);
  assert.ok(camera.x >= 0);
});

test('canvas context includes duplicate and foreign-space artboards, without truncating a long reference', () => {
  let state = add(add(createState('canvas'), 'a', target, dimensions), 'b', { ...target, space: { ...target.space, id: 'other', name: 'Other' } }, dimensions);
  state = change(state, 'a', { view: { ...ready, reference: 'a'.repeat(5000) } });
  const value = cleanView(report(state))!;
  assert.equal((value.artboards as unknown[]).length, 2);
  assert.match(reference(state), /Other/);
  assert.ok((value.text as string).length > 5000);
  assert.throws(() => cleanView({ ...report(state), selected: 'missing' }), /selected/);
  assert.throws(() => cleanView({ ...report(state), text: 'x'.repeat(100001) }), /no artboards were truncated/);
});

function setup() {
  const callbacks: { view(info: ViewInfo): void; focus(): void }[] = [];
  const calls: string[][] = [], disposed: number[] = [];
  const controller = createController({
    mount(_board, handlers) {
      const at = callbacks.length; callbacks.push(handlers); calls.push([]);
      return { element: new EventTarget() as HTMLIFrameElement,
        async request(command) { calls[at]!.push(command); return null; },
        async capture() { throw new Error('unused'); }, dispose() { disposed.push(at); },
      } satisfies Runtime;
    }, render() {}, host() {}, error(error) { throw error; },
  });
  return { controller, callbacks, calls, disposed };
}

test('replacing content retains the instance ID and discards late messages from its disposed renderer', () => {
  const { controller, callbacks, disposed } = setup();
  const id = controller.create(target, dimensions); callbacks[0]!.view(ready);
  controller.replace(id, { ...target, src: 'dashboard.html' });
  callbacks[0]!.view(ready);
  assert.equal(controller.snapshot().artboards[0]!.target.src, 'dashboard.html');
  assert.equal(controller.snapshot().artboards[0]!.id, id);
  assert.equal(controller.snapshot().artboards[0]!.view, null);
  assert.deepEqual(disposed, [0]);
  controller.close(id); assert.deepEqual(disposed, [0, 1]);
});

test('capture freezes all ready artboards, prevents structural changes, and unlocks after a failure', async () => {
  const { controller, callbacks, calls } = setup();
  const a = controller.create(target, dimensions), b = controller.create(target, dimensions);
  callbacks.forEach(c => c.view(ready));
  await assert.rejects(controller.capture(async snapshot => {
    assert.equal(snapshot.artboards.length, 2);
    assert.throws(() => controller.create(target, dimensions), /capture/);
    assert.throws(() => controller.close(a), /capture/);
    await assert.rejects(controller.command(b, 'reload'), /capture/);
    throw new Error('capture failed');
  }), /capture failed/);
  assert.deepEqual(calls, [['lock', 'unlock'], ['lock', 'unlock']]);
  await controller.command(b, 'reload');
  controller.dispose();
});

const fitSize = { key: 'fit', label: 'Fit', icon: 'minimize-2', button: true, kind: 'fit' as const, width: null, height: null };
const sidebarSize = { key: 'sidebar', label: 'Sidebar', icon: 'panel-left', button: true, kind: 'fixed' as const, width: 340, height: 'fill' as const };
const mobileSize = { key: 'mobile', label: 'Mobile', icon: 'smartphone', button: true, kind: 'fixed' as const, width: 393, height: 852 };

test('artboards range from 1 to 8192 CSS pixels and accept any length on a filled axis', () => {
  assert.deepEqual(size(1, 1), { width: 1, height: 1 });
  assert.throws(() => size(0, 10), /between 1 and 8192/);
  assert.throws(() => size(10, 8193), /between 1 and 8192/);
  assert.ok(supportsSize({ width: 340, height: 1234 }, [sidebarSize]));
  assert.ok(!supportsSize({ width: 341, height: 1234 }, [sidebarSize, mobileSize]));
  assert.ok(supportsSize({ width: 5, height: 5 }, [fitSize]));
  assert.ok(supportsSize({ width: 5, height: 5 }));
});

test('an artboard a page can’t be shown at takes the page’s first size, a filled axis at the canvas’s room', async () => {
  const requests: unknown[] = [];
  const callbacks: { view(info: ViewInfo): void }[] = [];
  const controller = createController({
    mount(_board, handlers) {
      callbacks.push(handlers);
      return { element: new EventTarget() as HTMLIFrameElement,
        async request(command, value) { if (command === 'size') requests.push(value); return null; },
        async capture() { throw new Error('unused'); }, dispose() {} } satisfies Runtime;
    }, render() {}, host() {}, error(error) { throw error; }, available: () => ({ width: 1000, height: 700 }),
  });
  const id = controller.create(target, dimensions);
  callbacks[0]!.view({ ...ready, sizes: [sidebarSize, mobileSize] });
  assert.deepEqual(controller.snapshot().artboards[0]!.size, { width: 340, height: 700 });
  assert.deepEqual(requests, [{ width: 340, height: 700 }]);
  await assert.rejects(controller.command(id, 'size', { width: 500, height: 500 }), /does not support/);
  await controller.command(id, 'size', { width: 393, height: 852 });
  assert.deepEqual(controller.snapshot().artboards[0]!.size, { width: 393, height: 852 });
  controller.dispose();
});
