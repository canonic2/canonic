import test from 'node:test';
import assert from 'node:assert/strict';
import { createPool } from './pool.ts';

test('native producers share one source and codec, isolate other sources/codecs, and preserve subscribed peers on stop', async () => {
  const producers: { subscribers: number; stopped: boolean; closed: boolean }[] = [];
  const pool = createPool(() => {
    const state = { subscribers: 0, stopped: false, closed: false }; producers.push(state);
    return { async start() { return { token: producers.indexOf(state) }; }, stop() { state.stopped = true; }, async close() { state.closed = true; }, subscribers() { return state.subscribers; }, accept() { return false; }, acceptHttp() { return false; } };
  });
  try {
    assert.deepEqual(await pool.start({ id: 'a', codec: 'jpeg' }), await pool.start({ id: 'a', codec: 'jpeg' }));
    await pool.start({ id: 'a', codec: 'h264' }); await pool.start({ id: 'b', codec: 'jpeg' });
    assert.equal(producers.length, 3);
    producers[0]!.subscribers = 1;
    pool.stop('a');
    assert.equal(producers[0]!.stopped, false);
    assert.equal(producers[1]!.stopped, true);
    assert.equal(producers[2]!.stopped, false);
  } finally { await pool.close(); }
  assert.equal(producers[0]!.closed, true); assert.equal(producers[2]!.closed, true);
  await assert.rejects(pool.start({ id: 'c' }), /closed/);
});
