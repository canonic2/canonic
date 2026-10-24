import test from 'node:test';
import assert from 'node:assert/strict';
import { createPreviewSessions } from './controller.js';

const page = {
  target: {
    space: { id: 'acme', name: 'Acme', url: 'http://example.com' },
    src: 'page',
    scenario: null,
    lens: 'storybook',
  },
  size: { width: 1000, height: 700 },
  pageLabel: 'Page',
};
const declared = {
  ...page,
  target: { ...page.target, src: 'feedback' },
  pageLabel: 'Feedback',
  artboards: ['info', 'warning'].map((id) => ({ id, label: id, width: 320, height: 120 })),
};
function fixture() {
  const visible = new Map();
  const parked = new Map();
  const closed = [],
    commands = [],
    concealed = [];
  let serial = 0,
    busy = false,
    fetched = 0;
  let failAt;
  let displayed = [];
  const sessions = createPreviewSessions({
    visible: () => [...visible.values()],
    displayed: () => displayed,
    create(target) {
      const id = String(++serial);
      if (serial === failAt) throw new Error('Renderer could not mount');
      visible.set(id, { id, target, initialized: true });
      return id;
    },
    choose(id) {
      assert.ok(visible.has(id));
    },
    close(id) {
      closed.push(id);
      visible.delete(id);
      parked.delete(id);
    },
    park(id) {
      parked.set(id, visible.get(id));
      visible.delete(id);
    },
    resume(id) {
      visible.set(id, parked.get(id));
      parked.delete(id);
    },
    conceal(id, value) {
      concealed.push([id, value]);
    },
    async command(id, command, value) {
      commands.push({ id, command, value });
    },
    busy: () => busy,
    async stories() {
      fetched++;
      return ['story'];
    },
  });
  return {
    sessions,
    visible,
    parked,
    closed,
    commands,
    concealed,
    setBusy(value) {
      busy = value;
    },
    fetched: () => fetched,
    failAt(value) {
      failAt = value;
    },
    display(ids) {
      displayed = ids;
    },
  };
}
void test('ordinary stories reuse one renderer; only authored artboards create multiple instances', async () => {
  const f = fixture();
  await f.sessions.activate(page);
  await f.sessions.activate({ ...page, target: { ...page.target, scenario: 'secondary' } });
  assert.equal(f.visible.size, 1);
  assert.equal(f.commands[0]?.id, '1');
  assert.equal(f.commands[0]?.command, 'navigate');
  assert.deepEqual(f.closed, []);
  await f.sessions.activate(declared);
  assert.equal(f.parked.has('1'), true);
  assert.deepEqual(
    [...f.visible.values()].map((b) => b.target.label),
    ['Feedback / info', 'Feedback / warning'],
  );
  await f.sessions.activate(page);
  assert.deepEqual([...f.visible.keys()], ['1']);
  assert.deepEqual(f.concealed, [
    ['1', true],
    ['1', false],
  ]);
  assert.equal(f.commands.at(-1)?.id, '1');
  f.sessions.dispose();
});

void test('outgoing artboards survive until presentation commits or the grace period expires', async () => {
  const f = fixture();
  await f.sessions.activate(page);
  f.display(['1']);
  await f.sessions.activate(declared);
  assert.ok(f.parked.has('1'));
  assert.deepEqual(f.concealed, [], 'the displayed ordinary renderer remains visible');
  f.sessions.finish();
  assert.deepEqual(f.concealed, [['1', true]]);
  f.display(['2', '3']);
  await f.sessions.activate(page);
  assert.deepEqual(f.closed, [], 'displayed declared renderers are parked during replacement');
  assert.ok(f.parked.has('2') && f.parked.has('3'));
  f.sessions.finish();
  assert.deepEqual(f.closed, ['2', '3']);
  assert.deepEqual([...f.visible.keys()], ['1']);
  f.sessions.dispose();
});

void test('rapid replacements discard unpublished renderers and keep the original outgoing view', async () => {
  const f = fixture();
  await f.sessions.activate(page);
  f.display(['1']);
  await f.sessions.activate(declared);
  await f.sessions.activate(declared);
  assert.deepEqual(f.closed, ['2', '3']);
  assert.ok(f.parked.has('1'));
  assert.deepEqual(f.concealed, []);
  await f.sessions.activate(page);
  f.sessions.finish();
  assert.deepEqual(
    f.concealed,
    [['1', false]],
    'resuming the outgoing view cancels its deferred concealment',
  );
  f.sessions.dispose();
});
void test('parked sessions expire after fifteen minutes and wait while review is holding the canvas', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const f = fixture();
  t.after(() => f.sessions.dispose());
  await f.sessions.activate(page);
  await f.sessions.activate(declared);
  f.setBusy(true);
  t.mock.timers.tick(15 * 60 * 1000);
  assert.ok(f.parked.has('1'));
  assert.deepEqual(f.closed, []);
  f.setBusy(false);
  t.mock.timers.tick(1000);
  assert.deepEqual(f.closed, ['1']);
  assert.equal(f.visible.size, 2);
});
void test('refresh reaches visible and parked sessions; disposal cancels expiry and catalog reuse', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const f = fixture();
  await f.sessions.activate(page);
  await f.sessions.activate(declared);
  await Promise.all([f.sessions.stories('sb', 'Inputs'), f.sessions.stories('sb', 'Inputs')]);
  assert.equal(f.fetched(), 1);
  await f.sessions.refresh();
  assert.deepEqual(f.commands.map((c) => c.id).sort(), ['1', '2', '3']);
  await f.sessions.stories('sb', 'Inputs');
  assert.equal(f.fetched(), 2);
  f.sessions.dispose();
  f.sessions.dispose();
  t.mock.timers.tick(20 * 60 * 1000);
  assert.deepEqual(f.closed, ['1']);
  await assert.rejects(f.sessions.activate(page), /closed/);
  await assert.rejects(f.sessions.stories('sb', 'Inputs'), /closed/);
});

void test('partial artboard creation releases newly created instances and retains the ordinary session for recovery', async () => {
  const f = fixture();
  await f.sessions.activate(page);
  f.failAt(3);
  await assert.rejects(f.sessions.activate(declared), /could not mount/);
  assert.equal(f.visible.size, 0);
  assert.deepEqual(f.closed, ['2']);
  assert.equal(f.parked.has('1'), true);
  await f.sessions.activate(page);
  assert.deepEqual([...f.visible.keys()], ['1']);
  f.sessions.dispose();
});
