import test from 'node:test';
import assert from 'node:assert/strict';
import { createNavigation, shownIterations } from './controller.js';

const selection = (src) => ({
  src,
  label: src,
  pageLabel: src,
  scenario: null,
  lens: 'design',
  docs: false,
  actions: false,
  size: {
    key: 'desktop',
    label: 'Desktop',
    icon: 'monitor',
    button: true,
    kind: 'fixed',
    width: 1000,
    height: 700,
  },
  select() {},
});
function deferred() {
  let resolve, reject;
  const promise = new Promise((yes, no) => {
    resolve = yes;
    reject = no;
  });
  return { promise, resolve, reject };
}
function report(navigation, boards) {
  navigation.observe({
    artboards: boards.map((board) => ({
      ...board,
      target: board.target || {},
      view: board.view && { ...board.view, navigation: navigation.state.get().id },
    })),
  });
  return navigation.presentation();
}
void test('fast retained returns stay busy without showing loading feedback or delaying visibility', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  let paints = 0;
  const navigation = createNavigation({
    ready: Promise.resolve(),
    busy: () => false,
    present() {
      paints++;
    },
    fit() {},
    async activate() {},
    async settle() {},
  });
  t.after(() => navigation.dispose());
  navigation.begin();
  await navigation.show(selection('returned'));
  const board = { view: { ...selection('returned'), ready: false } };
  assert.deepEqual(report(navigation, [board]), {
    visible: false,
    pending: true,
    loading: false,
  });
  t.mock.timers.tick(22);
  t.mock.timers.tick(128);
  assert.equal(
    report(navigation, [board]).loading,
    false,
    'a page ready at 150 ms still avoids the loader',
  );
  board.view.ready = true;
  assert.deepEqual(report(navigation, [board]), {
    visible: true,
    pending: false,
    loading: false,
  });
  navigation.complete();
  const before = paints;
  t.mock.timers.tick(200);
  assert.equal(paints, before, 'the completed selection cannot reveal a late loader');
});

void test('slow selections show loading feedback and replacement, failure and disposal cancel its timer', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  let paints = 0;
  const navigation = createNavigation({
    ready: Promise.resolve(),
    busy: () => false,
    present() {
      paints++;
    },
    fit() {},
    async activate() {},
    async settle() {},
  });
  t.after(() => navigation.dispose());
  navigation.begin();
  t.mock.timers.tick(199);
  assert.equal(navigation.presentation([]).loading, false);
  navigation.begin();
  t.mock.timers.tick(1);
  assert.equal(navigation.presentation([]).loading, false, 'the previous deadline is cancelled');
  t.mock.timers.tick(199);
  assert.deepEqual(navigation.presentation([]), { visible: false, pending: true, loading: true });
  navigation.begin();
  assert.equal(navigation.presentation([]).loading, false);
  navigation.fail();
  let before = paints;
  t.mock.timers.tick(200);
  assert.equal(paints, before);
  navigation.begin();
  navigation.dispose();
  before = paints;
  t.mock.timers.tick(200);
  assert.equal(paints, before);
});
void test('resolving a destination after the grace deadline keeps loading until its renderer settles', async (t) => {
  t.mock.timers.enable({ apis: ['setTimeout'] });
  const navigation = createNavigation({
    ready: Promise.resolve(),
    busy: () => false,
    present() {},
    fit() {},
    async activate() {},
    async settle() {},
  });
  t.after(() => navigation.dispose());
  navigation.begin();
  const id = navigation.state.get().id;
  t.mock.timers.tick(200);
  await navigation.show(selection('one'));
  assert.equal(navigation.state.get().id, id);
  assert.equal(navigation.presentation().loading, true);
  assert.equal(
    report(navigation, [{ target: {}, view: { ...selection('one'), ready: true } }]).visible,
    true,
  );
});
void test('retained destinations consume the requested size once, after matching metadata arrives', async () => {
  const navigation = createNavigation({
    ready: Promise.resolve(),
    busy: () => false,
    present() {},
    fit() {},
    async activate() {},
    async settle() {},
  });
  const docs = { ...selection('page'), lens: 'docs', docs: true };
  await navigation.show(docs);
  assert.equal(navigation.takeSize(docs), null, 'Docs owns its reading viewport');
  const story = { ...selection('page'), scenario: 'default', lens: 'storybook' };
  await navigation.show(story);
  assert.equal(navigation.takeSize(docs), null, 'outgoing Docs cannot consume the Storybook size');
  assert.equal(navigation.takeSize({ ...story, scenario: 'previous' }), null);
  assert.equal(navigation.takeSize(story), story.size);
  assert.equal(
    navigation.takeSize(story),
    null,
    'later content measurement is not overwritten by an old size intent',
  );
  const mobile = { ...story.size, key: 'mobile', width: 393, height: 852 };
  navigation.resize(mobile);
  assert.equal(navigation.takeSize(story), mobile, 'a user size pick replaces the route preset');
  assert.equal(navigation.takeSize(story), null);
  await navigation.show({ ...story, size: mobile });
  assert.equal(
    navigation.takeSize(story),
    mobile,
    'a size-only route still applies without replacing the session',
  );
  await navigation.show({ ...story, artboards: [{ id: 'one', width: 320, height: 120 }] });
  assert.equal(
    navigation.takeSize(story),
    null,
    'explicit artboards keep their authored dimensions',
  );
  navigation.dispose();
  assert.equal(navigation.takeSize(story), null);
});
void test('the applied size stays available for refilling until a new size is requested', async () => {
  const navigation = createNavigation({
    ready: Promise.resolve(),
    busy: () => false,
    present() {},
    fit() {},
    async activate() {},
    async settle() {},
  });
  const page = { ...selection('page'), scenario: 'default', lens: 'design' };
  await navigation.show(page);
  assert.equal(navigation.appliedSize(page), null, 'nothing is applied before the size is taken');
  navigation.takeSize(page);
  assert.equal(navigation.appliedSize(page), page.size);
  assert.equal(navigation.appliedSize(page), page.size, 'reading it does not consume it');
  assert.equal(
    navigation.appliedSize({ ...page, scenario: 'other' }),
    null,
    'only the shown destination',
  );
  const fit = { ...page.size, key: 'fit', kind: 'fit' };
  navigation.resize(fit);
  assert.equal(navigation.appliedSize(page), null, 'a new pick waits to be taken first');
  navigation.takeSize(page);
  assert.equal(navigation.appliedSize(page), fit);
  await navigation.show({
    ...page,
    size: fit,
    artboards: [{ id: 'one', width: 320, height: 120 }],
  });
  navigation.takeSize(page);
  assert.equal(navigation.appliedSize(page), null, 'declared artboards keep their authored sizes');
  const docs = { ...selection('page'), lens: 'docs', docs: true };
  await navigation.show(docs);
  assert.equal(navigation.appliedSize(docs), null, 'Docs read at the canvas width');
});
void test('navigation tracks requested destinations separately and reuses repeated routes', async () => {
  const activations = [],
    settled = [];
  let fits = 0,
    paints = 0;
  const navigation = createNavigation({
    ready: Promise.resolve(),
    busy: () => false,
    present() {
      paints++;
    },
    fit() {
      fits++;
    },
    async activate(next) {
      activations.push(next.src);
    },
    async settle(next) {
      settled.push(next.src);
    },
  });
  navigation.begin();
  assert.equal(navigation.snapshot().selection, undefined);
  assert.equal(navigation.snapshot().awaitingSelection, true);
  await navigation.show(selection('one'));
  assert.equal(navigation.snapshot().selection?.src, 'one');
  navigation.observe({
    artboards: [
      {
        target: {},
        view: { ...selection('one'), navigation: navigation.state.get().id, ready: true },
      },
    ],
  });
  navigation.complete();
  assert.equal(navigation.snapshot().navigating, false);
  await navigation.show(selection('one'));
  assert.deepEqual(activations, ['one']);
  assert.deepEqual(settled, ['one', 'one']);
  assert.equal(fits, 1);
  assert.ok(paints > 0);
});
void test('a superseded activation cannot settle, fit, or replace the current destination with its error', async () => {
  const slow = deferred(),
    settled = [];
  let fits = 0;
  const navigation = createNavigation({
    ready: Promise.resolve(),
    busy: () => false,
    present() {},
    fit() {
      fits++;
    },
    activate: (next) => (next.src === 'slow' ? slow.promise : Promise.resolve()),
    async settle(next) {
      settled.push(next.src);
    },
  });
  const old = navigation.show(selection('slow'));
  await Promise.resolve();
  navigation.begin();
  await navigation.show(selection('new'));
  slow.reject(new Error('outdated failure'));
  await old;
  assert.equal(navigation.snapshot().selection?.src, 'new');
  assert.deepEqual(settled, ['new']);
  assert.equal(fits, 1);
});
void test('failed activation retries and shutdown invalidates a pending startup', async () => {
  let attempts = 0;
  const navigation = createNavigation({
    ready: Promise.resolve(),
    busy: () => false,
    present() {},
    fit() {},
    async activate() {
      if (++attempts === 1) throw new Error('unavailable');
    },
    async settle() {},
  });
  await assert.rejects(navigation.show(selection('one')), /unavailable/);
  await navigation.show(selection('one'));
  assert.equal(attempts, 2);
  const startup = deferred();
  let activated = false;
  const closing = createNavigation({
    ready: startup.promise,
    busy: () => false,
    present() {},
    fit() {},
    async activate() {
      activated = true;
    },
    async settle() {},
  });
  const pending = closing.show(selection('one'));
  closing.dispose();
  startup.resolve();
  await pending;
  assert.equal(activated, false);
  assert.equal(closing.snapshot().disposed, true);
});
void test('capture prevents navigation without accepting another destination', async () => {
  const navigation = createNavigation({
    ready: Promise.resolve(),
    busy: () => true,
    present() {},
    fit() {},
    async activate() {},
    async settle() {},
  });
  navigation.begin();
  assert.equal(navigation.snapshot().navigating, false);
  await assert.rejects(navigation.show(selection('one')), /capture/);
  assert.equal(navigation.snapshot().selection, undefined);
});

void test('repeated readiness is a no-op, stale reports cannot settle, and feedback subscriptions end with their owner', async () => {
  const navigation = createNavigation({
    ready: Promise.resolve(),
    busy: () => false,
    present() {},
    fit() {},
    async activate() {},
    async settle() {},
  });
  const lifetime = new AbortController();
  let deliveries = 0;
  navigation.state.watch(navigation.select.feedback, () => deliveries++, {
    signal: lifetime.signal,
  });
  await navigation.show(selection('one'));
  const previous = navigation.state.get().id;
  const model = {
    artboards: [{ target: {}, view: { ...selection('one'), navigation: previous, ready: true } }],
  };
  navigation.observe(model);
  const committed = navigation.state.get();
  navigation.observe(model);
  assert.equal(
    navigation.state.get(),
    committed,
    'a renderer can repeat its current report without publishing again',
  );
  await navigation.show(selection('one'));
  navigation.observe(model);
  assert.equal(
    navigation.state.get().phase,
    'pending',
    'matching destination with an old identity cannot settle',
  );
  const current = navigation.state.get().id;
  assert.notEqual(current, previous);
  assert.equal(navigation.adopt(current, undefined, selection('other')), false);
  navigation.captureStarted();
  await assert.rejects(navigation.show(selection('other')), /capture/);
  navigation.captureEnded();
  assert.equal(navigation.adopt(previous, 'renderer:late', selection('other')), false);
  assert.equal(navigation.adopt(current, 'renderer:new', selection('other')), true);
  assert.equal(navigation.state.get().requested.src, 'other');
  lifetime.abort();
  const before = deliveries;
  navigation.clear();
  assert.equal(deliveries, before);
  navigation.dispose();
});

void test('presentation accepts only the requested preview or the complete declared artboard group', async () => {
  const navigation = createNavigation({
    ready: Promise.resolve(),
    busy: () => false,
    present() {},
    fit() {},
    async activate() {},
    async settle() {},
  });
  const next = selection('one');
  const board = {
    target: { src: 'one', scenario: null, lens: 'design' },
    view: { src: 'one', scenario: null, lens: 'design', ready: true, problem: null },
  };
  navigation.begin();
  assert.deepEqual(report(navigation, [board]), {
    visible: false,
    pending: true,
    loading: false,
  });
  await navigation.show(next);
  assert.equal(report(navigation, [board]).visible, true);
  await navigation.show(next);
  assert.equal(
    report(navigation, [{ ...board, view: { ...board.view, src: 'old' } }]).visible,
    false,
  );
  assert.equal(
    report(navigation, [{ ...board, view: { ...board.view, ready: false } }]).visible,
    false,
  );
  const declared = {
    ...next,
    artboards: [{ id: 'info', label: 'Info', width: 320, height: 120 }],
  };
  board.target.artboard = 'info';
  await navigation.show(declared);
  assert.equal(report(navigation, [board]).visible, true);
  await navigation.show(declared);
  assert.equal(
    report(navigation, [{ ...board, view: { ...board.view, ready: false } }]).visible,
    false,
    'a declared group waits for its content rather than only its instance metadata',
  );
  assert.equal(
    report(navigation, [{ ...board, target: { ...board.target, artboard: 'wrong' } }]).visible,
    false,
  );
  assert.equal(
    report(navigation, [{ ...board, view: { ...board.view, scenario: 'old' } }]).visible,
    false,
  );
  navigation.dispose();
  assert.equal(report(navigation, [board]).visible, false);
});

void test('another iteration keeps the artboard group and waits for that artboard to show it', async () => {
  const activations = [],
    iterated = [];
  const navigation = createNavigation({
    ready: Promise.resolve(),
    busy: () => false,
    present() {},
    fit() {},
    async activate(next) {
      activations.push(next.iterations);
    },
    async iterate(next) {
      iterated.push(next.iterations);
    },
    async settle() {},
  });
  const artboards = [
    { id: 'info', label: 'Info', width: 320, height: 120 },
    { id: 'warning', label: 'Warning', width: 320, height: 120 },
  ];
  const view = { src: 'one', scenario: null, lens: 'design', ready: true, problem: null };
  const boards = (warning) => [
    { target: { ...view, artboard: 'info' }, view },
    { target: { ...view, artboard: 'warning' }, view: { ...view, iteration: warning } },
  ];
  await navigation.show({ ...selection('one'), artboards, iterations: {} });
  await navigation.show({ ...selection('one'), artboards, iterations: { warning: 'bold' } });
  assert.deepEqual(activations, [{}], 'the declared group is created once');
  assert.deepEqual(iterated, [{ warning: 'bold' }]);
  assert.equal(report(navigation, boards(null)).visible, false);
  assert.equal(report(navigation, boards('bold')).visible, true);
  navigation.dispose();
});

void test('the canvas reports every artboard’s iteration under the design lens, whichever is selected', () => {
  const boards = [{ target: { artboard: 'info' } }, { target: { artboard: 'warning' } }];
  const declared = { ...selection('one'), iterations: { warning: 'bold', stale: 'calm' } };
  assert.deepEqual(shownIterations(declared, boards), { warning: 'bold' });
  assert.deepEqual(shownIterations({ ...declared, lens: null }, boards), { warning: 'bold' });
  assert.deepEqual(
    shownIterations({ ...selection('one'), iterations: { '': 'illustrated' } }, [{ target: {} }]),
    { '': 'illustrated' },
  );
  assert.deepEqual(shownIterations({ ...declared, lens: 'storybook' }, boards), {});
  assert.deepEqual(shownIterations({ ...declared, docs: true }, boards), {});
  assert.deepEqual(shownIterations({ ...selection('one'), iterations: {} }, boards), {});
  assert.deepEqual(shownIterations(null, boards), {});
});
