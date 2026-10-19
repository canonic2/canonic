import test from 'node:test';
import assert from 'node:assert/strict';
import { createReview } from './controller.js';

function fixture() {
  const events = [];
  let deliveryFailure = false;
  const state = {
    id: 'canvas',
    revision: 0,
    selected: 'a',
    artboards: [
      {
        id: 'a',
        target: {
          space: { id: 'acme', name: 'Acme', url: 'http://example.com' },
          src: 'page',
          state: null,
          lens: null,
        },
        size: { width: 320, height: 120 },
        x: 0,
        y: 0,
        generation: 1,
        view: {
          src: 'page',
          state: null,
          lens: null,
          label: 'Page',
          hash: '#page',
          reference: 'Page',
          ready: true,
          problem: null,
          payload: {},
          states: [],
          lenses: [],
        },
      },
    ],
  };
  const runtime = {
    async request(name) {
      events.push(name);
    },
  };
  const review = createReview({
    available: () => true,
    hold() {
      events.push('hold');
      return () => {
        events.push('release');
      };
    },
    async capture(run) {
      events.push('lock');
      try {
        return await run(state, () => runtime);
      } finally {
        events.push('unlock');
      }
    },
    async acquire() {
      events.push('acquire');
      return { blob: new Blob(['image']), payload: { label: 'Canvas' } };
    },
    async save() {
      events.push('save');
      return '/tmp/canvas.jpg';
    },
    async handoff(payload) {
      assert.equal(payload.file, '/tmp/canvas.jpg');
      events.push('handoff');
      if (deliveryFailure) throw new Error('delivery failed');
    },
    download() {
      events.push('download');
    },
    notify() {
      events.push('notify');
    },
  });
  return {
    review,
    events,
    fail() {
      deliveryFailure = true;
    },
  };
}
void test('handoff clears captured annotations only after saving and delivering the frozen capture', async () => {
  const f = fixture();
  await f.review.run(true);
  assert.deepEqual(f.events, [
    'hold',
    'lock',
    'acquire',
    'save',
    'handoff',
    'clear-captured',
    'notify',
    'unlock',
    'release',
  ]);
  assert.equal(f.review.busy(), false);
});
void test('failed delivery keeps annotations and releases both locks; downloads do not deliver a handoff', async () => {
  const f = fixture();
  f.fail();
  await assert.rejects(f.review.run(true), /delivery failed/);
  assert.equal(f.events.includes('clear-captured'), false);
  assert.deepEqual(f.events.slice(-2), ['unlock', 'release']);
  assert.equal(f.review.busy(), false);
  f.events.length = 0;
  await f.review.run(false);
  assert.deepEqual(f.events, ['hold', 'lock', 'acquire', 'download', 'unlock', 'release']);
});
void test('one review owns the operation until it releases the capture boundary', async () => {
  const f = fixture();
  const running = f.review.run(false);
  assert.equal(f.review.busy(), true);
  await assert.rejects(f.review.run(false), /already running/);
  await running;
  assert.equal(f.review.busy(), false);
});
