import test from 'node:test';
import assert from 'node:assert/strict';
import { checkSelection } from '@canonic2/lattice/testing';
import { createPreviewControls } from './index.js';

const ready = (id = 'button', scenario = 'default') => ({
  event: 'ready',
  id,
  scenario,
  controls: { label: { type: 'text' } },
  inputs: { label: 'Continue' },
  actions: [],
  docs: 'Acme',
});

test('inputs, action logs and selection belong to their renderer, without mutating reports', () => {
  const sent = [];
  const controls = createPreviewControls({ send: (...args) => sent.push(args) });
  const report = ready();
  controls.receive('one', report);
  controls.choose('one');
  controls.toggle();
  controls.changeInput('label', 'Edited');
  controls.receive('one', { event: 'action', id: 'button', name: 'clicked', values: [1] });
  assert.equal(report.inputs.label, 'Continue');
  assert.deepEqual(report.actions, []);
  controls.receive('two', ready('other'));
  controls.choose('two');
  assert.equal(controls.select.opened.read(controls.state.get()), false);
  assert.equal(controls.select.model.read(controls.state.get()).inputs.label, 'Continue');
  controls.choose('one');
  assert.equal(controls.select.model.read(controls.state.get()).inputs.label, 'Edited');
  assert.equal(controls.select.model.read(controls.state.get()).actions.length, 1);
  controls.receive('one', { event: 'action', id: 'stale', name: 'wrong' });
  assert.equal(controls.select.model.read(controls.state.get()).actions.length, 1);
  controls.resetScenario();
  assert.deepEqual(sent, [
    ['one', { scenario: 'default', inputs: { label: 'Edited' } }],
    ['one', { command: 'reset', scenario: 'default', inputs: {} }],
  ]);
  assert.deepEqual(controls.select.model.read(controls.state.get()).actions, []);
  controls.receive('one', ready('button', 'secondary'));
  assert.equal(controls.select.model.read(controls.state.get()).inputs.label, 'Continue');
  controls.deselect();
  controls.toggle();
  assert.equal(controls.select.available.read(controls.state.get()), false);
  controls.choose('one');
  assert.equal(controls.select.available.read(controls.state.get()), true);
  controls.clear();
  assert.deepEqual(controls.state.get(), { instances: {}, selected: null, opened: false });
});

test('selections declare their dependencies; other renderers do not redraw selected controls', () => {
  const controls = createPreviewControls({ send() {} });
  const empty = controls.state.get();
  controls.receive('one', ready());
  controls.choose('one');
  const showing = controls.state.get();
  for (const [name, selection] of Object.entries(controls.select))
    checkSelection(name, selection, { empty, showing, open: { ...showing, opened: true } });
  const lifetime = new AbortController();
  let changes = 0;
  controls.state.watch(controls.select.model, () => changes++, { signal: lifetime.signal });
  controls.receive('two', ready('other'));
  assert.equal(changes, 1);
  for (let i = 0; i < 40; i++)
    controls.receive('one', { event: 'action', id: 'button', name: String(i) });
  assert.equal(controls.select.model.read(controls.state.get()).actions.length, 30);
  lifetime.abort();
  const before = changes;
  controls.clear();
  assert.equal(changes, before);
});

test('a command rejection reaches its caller and leaves the committed experiment visible', async () => {
  const controls = createPreviewControls({
    send: async () => {
      throw Error('Renderer closed');
    },
  });
  controls.receive('one', ready());
  controls.choose('one');
  await assert.rejects(controls.changeInput('label', 'Edited'), /Renderer closed/);
  assert.equal(controls.select.model.read(controls.state.get()).inputs.label, 'Edited');
});
