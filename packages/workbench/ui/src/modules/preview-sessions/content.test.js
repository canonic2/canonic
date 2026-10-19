import test from 'node:test';

import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createContentController } from './content.js';

const { mergeCollections } = createRequire(import.meta.url)('../../../../contracts/manifest.js');

const tick = () => new Promise((resolve) => setImmediate(resolve));
function deferred() {
  let resolve;
  let reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function fixture(t, hash = '#page~design') {
  const location = new URL('http://example.com/_workbench/content.html' + hash);
  const events = new EventTarget();
  const values = {
    window: events,
    location,
    innerWidth: 800,
    innerHeight: 600,
    history: {
      replaceState(_state, _unused, url) {
        location.href = new URL(url, location).href;
      },
    },
  };
  const restore = [];
  for (const [name, value] of Object.entries(values)) {
    const descriptor = Object.getOwnPropertyDescriptor(globalThis, name);
    Object.defineProperty(globalThis, name, { value, configurable: true });
    restore.push(() => {
      if (descriptor) Object.defineProperty(globalThis, name, descriptor);
      else Reflect.deleteProperty(globalThis, name);
    });
  }
  const config = {
    ok: true,
    sizes: [],
    pages: { page: {}, other: {} },
    implementations: {
      storybook: {
        key: 'storybook',
        kind: 'storybook',
        label: 'Storybook',
        url: 'http://storybook.example.com',
      },
    },
    collections: [
      {
        items: [
          {
            src: 'page',
            label: 'Authored page',
            states: [
              { id: 'default', label: 'Default' },
              { id: 'error', label: 'Error' },
            ],
            implementations: { storybook: { title: 'Page' } },
          },
          { src: 'other', label: 'Other page', implementations: { storybook: { title: 'Other' } } },
        ],
      },
    ],
    catalogCollections: [
      {
        items: [
          {
            src: 'page',
            label: 'Generated page',
            workbench: true,
            states: [
              { id: 'default', label: 'Default' },
              { id: 'error', label: 'Error' },
            ],
          },
        ],
      },
    ],
  };
  const shown = [];
  const catalogs = new Map();
  const classes = new Set();
  const viewport = {
    classList: {
      add: (name) => classes.add(name),
      remove: (name) => classes.delete(name),
    },
  };
  const surface = { dataset: {}, style: {} };
  let held = false,
    closed = 0;
  const controller = createContentController({
    root: 'http://example.com/',
    viewport,
    surface,
    held: () => held,
    loadConfig: async () => config,
    frames: {
      show(view) {
        shown.push(view);
      },
      ready: () => true,
      frame: () => ({}),
      suspend() {},
      storyEvent() {},
      close() {
        closed++;
      },
    },
    services: {
      mergeCollections,
      address: {
        parse(hash) {
          const [page, lens] = hash.replace(/^#/, '').split('~');
          const [src, state] = page.split(':');
          return { src: src, state: state || null, lens: lens || null };
        },
        write(target) {
          return (
            target.src +
            (target.state ? ':' + target.state : '') +
            (target.lens ? '~' + target.lens : '')
          );
        },
      },
      lenses: {
        url: () => 'http://example.com/page',
        storyUrl: (_lens, id) => 'http://storybook.example.com/iframe.html?id=' + id,
      },
      native: { show() {}, hide() {}, reload() {}, ready: () => false },
    },
    stories(_implementation, title) {
      const job = deferred();
      catalogs.set(title, job);
      return job.promise;
    },
  });
  t.after(() => {
    controller.dispose();
    restore.forEach((fn) => fn());
  });
  return {
    controller,
    shown,
    catalogs,
    classes,
    surface,
    events,
    location,
    hold() {
      held = true;
    },
    closed: () => closed,
  };
}

void test('content metadata comes from the authored model, without menu DOM', async (t) => {
  const f = fixture(t);
  f.controller.start();
  await tick();
  const info = f.controller.content.read();
  assert.equal(info.label, 'Authored page');
  f.controller.content.navigate({ src: 'page', state: null, lens: 'design' });
  await tick();
  assert.equal(
    f.shown.length,
    1,
    'reflecting the current destination into the outer shell cannot remount it',
  );
  assert.deepEqual(
    info.states.map((s) => [s.target, s.label]),
    [
      [null, 'Default'],
      ['error', 'Error'],
    ],
  );
  assert.deepEqual(
    info.lenses.map((l) => l.id),
    ['design', 'storybook'],
  );
  f.controller.content.pickState('1');
  await tick();
  assert.equal(f.shown.at(-1).state, 'error');
  assert.equal(f.controller.content.read().label, 'Authored page / Error');
  f.controller.content.size(320, 180);
  assert.equal(f.surface.style.width, '320px');
  assert.equal(f.surface.style.height, '180px');
  assert.throws(() => f.controller.content.navigate({}), /Invalid content target/);
});

void test('a delayed catalog cannot inherit outgoing readiness or complete a superseded view', async (t) => {
  const f = fixture(t);
  f.controller.start();
  await tick();
  assert.equal(f.controller.content.read().ready, true);
  f.controller.content.navigate({ src: 'page', lens: 'storybook' });
  assert.equal(f.controller.content.read().ready, false);
  assert.ok(f.classes.has('is-pending'));
  f.controller.renderFailed(f.shown[0], new Error('Outgoing mount failed'));
  assert.equal(
    f.controller.content.problem(),
    null,
    'outgoing render errors cannot fail a pending catalog destination',
  );
  const first = f.catalogs.get('Page');
  f.controller.content.action('actions', true);
  assert.equal(
    f.catalogs.get('Page'),
    first,
    'external interaction preference cannot restart a pending story',
  );
  f.controller.content.navigate({ src: 'other', lens: 'storybook' });
  const second = f.catalogs.get('Other');
  first.resolve([{ id: 'page--default', state: 'default', name: 'Stale story' }]);
  await tick();
  assert.equal(f.shown.length, 1);
  assert.equal(f.controller.content.read().label, 'Other page');
  assert.equal(f.controller.content.read().ready, false);
  second.resolve([{ id: 'other--default', state: 'default', name: 'Current story' }]);
  await tick();
  f.controller.settled();
  assert.equal(f.shown.length, 2);
  assert.equal(f.shown.at(-1).story.id, 'other--default');
  assert.equal(f.controller.content.read().ready, true);
  assert.equal(f.classes.has('is-pending'), false);
});

void test('failed lookups expose their destination and disposal rejects late completion', async (t) => {
  const f = fixture(t, '#page~storybook');
  f.controller.start();
  await tick();
  f.catalogs.get('Page').reject(new Error('Storybook unavailable'));
  await tick();
  assert.equal(f.controller.content.problem(), 'Storybook unavailable');
  assert.equal(f.controller.content.read().ready, false);
  f.controller.content.navigate({ src: 'other', lens: 'storybook' });
  f.controller.dispose();
  f.catalogs.get('Other').resolve([{ id: 'other--default', state: 'default', name: 'Late story' }]);
  await tick();
  assert.equal(f.shown.length, 0);
  assert.equal(f.closed(), 1);
});
