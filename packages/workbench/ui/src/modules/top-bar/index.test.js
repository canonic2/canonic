import test from 'node:test';
import assert from 'node:assert/strict';
import { checkSelection } from '@canonic2/lattice/testing';
import { createTopBar, createSizeControls } from './index.js';

const scenarios = [
  { id: 'default', label: 'Default' },
  { id: 'empty', label: 'Empty' },
];
const item = {
  src: 'card.html',
  label: 'Card',
  scenarios,
  implementations: { dev: { path: '/card' }, storybook: { title: 'Card' } },
};
const implementations = {
  dev: { key: 'dev', label: 'Dev', kind: 'url' },
  storybook: { key: 'storybook', label: 'Storybook', kind: 'storybook' },
};

test('top-bar owns route choices and preferences; lens picks retain iterations and scenario picks clear them', () => {
  const routes = [],
    remembered = [];
  let reloads = 0;
  const bar = createTopBar({
    writeHash: (...args) => routes.push(args),
    remember: (...args) => remembered.push(args),
    size: () => 'laptop',
    authoredLabel: () => 'Preview',
    actionsChanged: () => reloads++,
  });
  bar.setPage(item.src);
  bar.setScenario('empty');
  bar.setExample('example');
  bar.setIterations({ warning: 'bold' });
  bar.showLenses(item, implementations, null);
  bar.chooseLens('storybook');
  assert.deepEqual(routes.at(-1), [
    item.src,
    'empty',
    'laptop',
    'storybook',
    'example',
    { warning: 'bold' },
  ]);
  bar.showScenarios(item, null, scenarios);
  bar.chooseScenario(null);
  assert.deepEqual(routes.at(-1), [item.src, null, 'laptop', null, undefined, undefined]);
  const count = routes.length;
  bar.chooseScenario('missing');
  bar.chooseLens('missing');
  assert.equal(routes.length, count);
  bar.chooseActions(true);
  assert.equal(reloads, 1);
  assert.equal(bar.state.get().actionsOn, true);
  assert.deepEqual(remembered.at(-1), ['actions', 'on']);
  bar.showLenses(item, implementations, implementations.dev);
  assert.deepEqual(bar.select.actions.read(bar.state.get()), {
    checked: true,
    disabled: true,
    hint: 'always on here: Dev serves this page, not the workbench, so nothing inside it is switched off',
  });
  bar.chooseActions(false);
  assert.equal(reloads, 1);
  assert.equal(bar.state.get().actionsOn, true);
});

test('top-bar selections declare their fields and unchanged or unrelated updates notify no other controls', () => {
  const bar = createTopBar();
  const empty = bar.state.get();
  bar.setPage(item.src);
  bar.showPage(item);
  bar.showLenses(item, implementations, implementations.dev);
  bar.showScenarios(item, null, scenarios);
  const active = bar.state.get();
  for (const [name, selection] of Object.entries(bar.select))
    checkSelection(name, selection, { empty, active });
  let lenses = 0,
    scenarioChanges = 0;
  const lifetime = new AbortController();
  bar.state.watch(bar.select.lenses, () => lenses++, { signal: lifetime.signal });
  bar.state.watch(bar.select.scenarios, () => scenarioChanges++, { signal: lifetime.signal });
  bar.showLenses(item, { ...implementations }, { ...implementations.dev });
  bar.showScenarios(
    item,
    null,
    scenarios.map((scenario) => ({ ...scenario })),
  );
  bar.setActions(true);
  bar.setOpenLink('https://example.com/card');
  assert.equal(lenses, 1);
  assert.equal(scenarioChanges, 1);
  lifetime.abort();
  bar.showLenses(null, {}, null);
  assert.equal(lenses, 1);
});

test('source choices expose missing paths without opening them and story choices use their scenario address', async () => {
  const opened = [],
    routes = [];
  const bar = createTopBar({
    openSource: (...args) => opened.push(args),
    writeHash: (...args) => routes.push(args),
  });
  bar.setPage(item.src);
  bar.showLenses(item, implementations, implementations.storybook);
  bar.showSources(
    item,
    {
      design: '/acme/card.html',
      code: [
        { implementation: 'dev', path: '/acme/card.ts', relative: 'card.ts', exists: false },
        { implementation: 'dev', path: '/acme/other.ts', relative: 'other.ts', exists: true },
      ],
    },
    implementations,
  );
  const sources = bar.select.sources.read(bar.state.get()).rows;
  await bar.chooseSource(sources[1].id);
  assert.deepEqual(opened, []);
  await bar.chooseSource(sources[2].id);
  assert.deepEqual(opened.pop(), [item.src, '/acme/other.ts']);
  await bar.chooseSource('design');
  assert.deepEqual(opened, [[item.src, null]]);
  bar.showStories([{ id: 'card--primary', name: 'Primary', scenario: 'primary' }], {
    id: 'card--primary',
  });
  bar.chooseScenario('primary');
  assert.deepEqual(routes[0], [item.src, 'primary', undefined, 'storybook', undefined, undefined]);
});

test('size workflows recover after rejection, block duplicate writes and release late completion', async () => {
  let resolve, reject;
  const written = [],
    added = [],
    reported = [];
  const sizes = createSizeControls({
    context: () => ({ page: 'card.html', docs: false, allowPageOnly: true, notice: 'Acme' }),
    sizes: () => [],
    choose() {},
    updated() {},
    post: (body) => {
      written.push(body);
      return new Promise((yes, no) => {
        resolve = yes;
        reject = no;
      });
    },
    added: (...args) => added.push(args),
    report: (error) => reported.push(error),
  });
  for (const [name, selection] of Object.entries(sizes.select))
    checkSelection(name, selection, { initial: sizes.state.get() });
  sizes.openAdd();
  const detail = { name: 'Acme', width: 320, height: 180 };
  const first = sizes.add(detail);
  sizes.closeAdd();
  await sizes.add(detail);
  assert.equal(written.length, 1);
  assert.equal(sizes.state.get().add.open, true);
  reject(Error('Write refused'));
  await first;
  assert.equal(sizes.state.get().add.pending, false);
  assert.equal(sizes.state.get().add.error, 'Write refused');
  const second = sizes.add(detail);
  resolve({ key: 'acme' });
  await second;
  assert.equal(sizes.state.get().add.open, false);
  assert.equal(added.length, 1);
  assert.equal(written[0].size.page, 'card.html');
  sizes.openEdit();
  const late = sizes.save([]);
  const snapshot = sizes.state.get();
  sizes.dispose();
  const failure = Error('Late failure');
  reject(failure);
  await late;
  assert.equal(sizes.state.get(), snapshot);
  assert.deepEqual(reported, [failure]);
});
