import test from 'node:test';
import assert from 'node:assert/strict';
import { createPreviewControls } from '../../modules/preview-controls/index.js';
import { bindPreviewControls } from './preview-bindings.js';

const ready = { event: 'ready', id: 'acme', controls: { label: { type: 'text' } }, inputs: {} };
class Target extends EventTarget {
  hidden = false;
  setAttribute() {}
}

test('failed setup releases earlier subscriptions and a pre-aborted binding does no work', () => {
  const controls = createPreviewControls({ send() {} });
  const lifetime = new AbortController();
  let assignments = 0;
  const panel = new Target();
  Object.defineProperty(panel, 'model', {
    set() {
      assignments++;
    },
  });
  const button = new Target();
  Object.defineProperty(button, 'hidden', {
    set() {
      throw Error('Unavailable target');
    },
  });
  const ports = { controls, panel, button, signal: lifetime.signal, report() {} };
  assert.throws(() => bindPreviewControls(ports), /Unavailable target/);
  assert.equal(assignments, 1, 'the first watcher ran before setup failed');
  controls.receive('one', ready);
  controls.choose('one');
  assert.equal(assignments, 1, 'the earlier watcher was released');
  lifetime.abort();
  bindPreviewControls(ports);
  assert.equal(assignments, 1);
});

test('action failures use the live presenter, fall back after teardown and contain presenter errors', async (t) => {
  const platform = [];
  const original = Object.getOwnPropertyDescriptor(globalThis, 'reportError');
  Object.defineProperty(globalThis, 'reportError', {
    configurable: true,
    value: (error) => platform.push(error),
  });
  t.after(() => {
    if (original) Object.defineProperty(globalThis, 'reportError', original);
    else delete globalThis.reportError;
  });
  const lifetime = new AbortController();
  const panel = new Target(),
    button = new Target();
  const shown = [];
  let reject, reporterFailure;
  const controls = createPreviewControls({
    send: () =>
      new Promise((_, fail) => {
        reject = fail;
      }),
  });
  controls.receive('one', ready);
  controls.choose('one');
  bindPreviewControls({
    controls,
    panel,
    button,
    signal: lifetime.signal,
    report(error) {
      if (reporterFailure) throw reporterFailure;
      shown.push(error);
    },
  });
  const change = () =>
    panel.dispatchEvent(
      new CustomEvent('wb-preview-input', { detail: { name: 'label', value: 'Edited' } }),
    );
  const settle = () => new Promise((resolve) => setImmediate(resolve));
  change();
  const live = Error('Live command failed');
  reject(live);
  await settle();
  assert.deepEqual(shown, [live]);
  reporterFailure = Error('Presenter failed');
  change();
  reject(Error('Another command failed'));
  await settle();
  assert.deepEqual(platform, [reporterFailure]);
  change();
  lifetime.abort();
  const late = Error('Late command failed');
  reject(late);
  await settle();
  assert.deepEqual(platform, [reporterFailure, late]);
  assert.deepEqual(shown, [live], 'the view is not called after teardown');
  reject = null;
  change();
  assert.equal(reject, null, 'teardown also releases intent listeners');
});
