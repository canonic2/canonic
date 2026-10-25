import test from 'node:test';
import assert from 'node:assert/strict';
import { initial, transition, feedback, matches } from './state.js';
import { checkSelection } from '../../../../node_modules/@canonic2/lattice/src/testing/index.js';
import { createRendererNavigation } from './renderer.js';

const destination = (src = 'page') => ({
  src,
  scenario: null,
  lens: 'design',
  artboards: [{ id: 'one' }, { id: 'two' }],
  iterations: { two: 'bold' },
});
const view = (src = 'page', iteration = null) => ({
  src,
  scenario: null,
  lens: 'design',
  iteration,
  ready: true,
});
const apply = (state, event) => ({ ...state, ...transition(state, event) });

test('a renderer settles its own declared iteration and re-identification waits for a current report', () => {
  const renderer = createRendererNavigation('canvas:one', destination(), 'two');
  renderer.report(view('page', 'bold'));
  assert.equal(renderer.state.get().phase, 'shown');
  assert.equal(renderer.state.get().displayed.id, 'canvas:one');
  renderer.identify('canvas:two');
  assert.equal(renderer.state.get().phase, 'pending');
  renderer.report(view('page', 'bold'));
  assert.equal(renderer.state.get().displayed.id, 'canvas:two');
});

test('feedback declares every field it reads in every phase', () => {
  checkSelection(
    'navigation feedback',
    feedback,
    Object.fromEntries(
      ['idle', 'pending', 'loading', 'shown', 'failed'].flatMap((phase) =>
        [false, true].map((capturing) => [
          phase + capturing,
          { ...initial(), phase, capturing, problem: phase === 'failed' ? 'Unavailable' : null },
        ]),
      ),
    ),
  );
});

test('mixed slot results preserve ready content, match iterations and ignore superseded reports', () => {
  let state = apply(initial(), { type: 'picked', id: 'a', destination: destination() });
  state = apply(state, { type: 'ready', id: 'a', slot: 'one', view: view() });
  assert.equal(state.phase, 'pending');
  assert.deepEqual(transition(state, { type: 'ready', id: 'a', slot: 'two', view: view() }), {});
  state = apply(state, { type: 'failed', id: 'a', slot: 'two', problem: 'Unavailable' });
  assert.equal(state.phase, 'shown');
  assert.equal(state.slots[0].status, 'ready');
  assert.equal(feedback.read(state).inert, false);
  const replacement = apply(state, { type: 'picked', id: 'b', destination: destination('next') });
  assert.equal(
    replacement.displayed,
    state.displayed,
    'outgoing model survives only the grace period',
  );
  state = apply(replacement, { type: 'deadline', id: 'b' });
  assert.equal(state.displayed, null);
  for (const event of [
    { type: 'deadline' },
    { type: 'ready', slot: 'one', view: view() },
    { type: 'failed', problem: 'Late' },
    { type: 'navigated', from: 'a', destination: destination() },
  ])
    assert.deepEqual(transition(state, { ...event, id: 'a' }), {});
});

test('all failed slots and destination failures settle, while capture independently blocks input', () => {
  let state = apply(initial(), { type: 'picked', id: 'a', destination: destination() });
  for (const slot of ['one', 'two'])
    state = apply(state, { type: 'failed', id: 'a', slot, problem: slot });
  assert.equal(state.phase, 'failed');
  assert.equal(state.displayed.id, 'a');
  state = apply(state, { type: 'captureStarted' });
  assert.equal(feedback.read(state).inert, true);
  state = apply(state, { type: 'captureEnded' });
  assert.equal(feedback.read(state).inert, false);
  state = apply(state, { type: 'picked', id: 'b', destination: destination() });
  state = apply(state, { type: 'failed', id: 'b', problem: 'Activation failed' });
  assert.equal(state.phase, 'failed');
  assert.equal(state.displayed, null);
});

test('generated event sequences preserve navigation invariants', () => {
  let seed = 72341;
  const random = (n) => {
    seed = (Math.imul(seed, 1664525) + 1013904223) >>> 0;
    return seed % n;
  };
  for (let run = 0; run < 100; run++) {
    let state = initial();
    for (let step = 0; step < 100; step++) {
      const id = random(3) ? state.id : 'stale';
      const slot = random(2) ? 'one' : 'two';
      const events = [
        { type: 'picked', id: `${run}:${step}`, destination: destination() },
        { type: 'deadline', id },
        { type: 'ready', id, slot, view: view('page', slot === 'two' ? 'bold' : null) },
        { type: 'ready', id, slot, view: view('wrong') },
        { type: 'failed', id, slot, problem: 'Slot failed' },
        { type: 'failed', id, problem: 'Destination failed' },
        { type: 'captureStarted' },
        { type: 'captureEnded' },
        { type: 'navigated', from: id, id: `renderer:${run}:${step}`, destination: destination() },
      ];
      const event = events[random(events.length)];
      const patch = transition(state, event);
      if (event.id !== state.id && !['picked', 'navigated'].includes(event.type) && 'id' in event)
        assert.deepEqual(patch, {});
      if (event.type === 'navigated' && event.from !== state.id) assert.deepEqual(patch, {});
      state = { ...state, ...patch };
      assert.equal(
        !feedback.read(state).inert,
        ['shown', 'failed'].includes(state.phase) && !state.capturing,
      );
      if (state.phase === 'loading') assert.equal(state.displayed, null);
      if (state.phase === 'shown') {
        assert.equal(state.displayed.id, state.id);
        assert.ok(state.slots.every((s) => s.status !== 'pending'));
        assert.ok(state.slots.some((s) => s.status === 'ready'));
        for (const slot of state.slots.filter((s) => s.status === 'ready'))
          assert.ok(matches(state.requested, slot.id, slot.view));
      }
      if (['pending', 'loading'].includes(state.phase) && state.requested)
        assert.ok(state.slots.some((s) => s.status === 'pending'));
      if (state.phase === 'failed')
        assert.ok(state.problem || state.slots.every((s) => s.status === 'failed'));
    }
  }
});
