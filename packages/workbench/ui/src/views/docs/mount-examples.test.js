import test from 'node:test';
import assert from 'node:assert/strict';
import { readFile } from 'node:fs/promises';
import vm from 'node:vm';

const source = await readFile(new URL('./mount-examples.js', import.meta.url), 'utf8');
function fixture(images = []) {
  const figure = { dataset: { wbExample: 'basic' }, querySelector: () => ({}) };
  const root = { querySelectorAll: () => [figure] };
  const mount = vm.runInNewContext(
    source.replace('export async function', 'async function') + '\nmountExamples;',
    {
      AbortController,
      setTimeout,
      clearTimeout,
      document: { images, fonts: { ready: Promise.resolve() } },
      window: { innerWidth: 800, innerHeight: 600 },
    },
  );
  return { root, mount, options: { page: 'acme.md', scenario: 'default', report() {} } };
}

test('stopping during an asynchronous mount aborts its context and releases the eventual cleanup once', async () => {
  const f = fixture();
  const controller = new AbortController();
  const cleaned = [];
  let release, entered, signal;
  const mounted = new Promise((resolve) => {
    entered = resolve;
  });
  const waiting = new Promise((resolve) => {
    release = resolve;
  });
  const work = f.mount(
    f.root,
    {
      examples: { basic: {} },
      environment: {
        setup: () => () => cleaned.push('environment'),
        ready: () => assert.fail('aborted mount must not become ready'),
      },
      adapter: {
        mount: async (_stage, _value, context) => {
          signal = context.signal;
          entered();
          await waiting;
          return () => cleaned.push('example');
        },
      },
    },
    { ...f.options, signal: controller.signal },
  );
  await mounted;
  controller.abort();
  assert.equal(signal.aborted, true);
  release();
  const result = await work;
  await result.dispose();
  await result.dispose();
  assert.deepEqual(cleaned, ['example', 'environment']);
});

test('a readiness failure cleans mounted examples and the environment before rejecting', async () => {
  const f = fixture();
  const cleaned = [];
  await assert.rejects(
    f.mount(
      f.root,
      {
        examples: { basic: {} },
        environment: {
          setup: () => () => cleaned.push('environment'),
          ready: () => {
            throw Error('Acme ready failed');
          },
        },
        adapter: { mount: () => () => cleaned.push('example') },
      },
      f.options,
    ),
    /Acme ready failed/,
  );
  assert.deepEqual(cleaned, ['example', 'environment']);
});

test('aborting an image readiness wait removes its listeners and does not wait for the timeout', async () => {
  const image = new EventTarget();
  image.complete = false;
  const listeners = new Set();
  const add = image.addEventListener.bind(image),
    remove = image.removeEventListener.bind(image);
  image.addEventListener = (type, listener, options) => {
    listeners.add(type);
    add(type, listener, options);
  };
  image.removeEventListener = (type, listener) => {
    listeners.delete(type);
    remove(type, listener);
  };
  const f = fixture([image]);
  const controller = new AbortController();
  let entered;
  const waiting = new Promise((resolve) => {
    entered = resolve;
  });
  const work = f.mount(
    f.root,
    { examples: { basic: {} }, adapter: { mount() {} } },
    {
      ...f.options,
      signal: controller.signal,
      phase: (name) => {
        if (name === 'readiness-fonts') entered();
      },
    },
  );
  await waiting;
  assert.equal(listeners.size, 2);
  controller.abort();
  const result = await work;
  assert.equal(listeners.size, 0);
  await result.dispose();
});
