const { test } = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const compilerFixture = require('../server/node/compiler-fixture.cjs');
const server = require('../server/node/server.js');
const chrome = require('../scripts/chrome.cjs');

const root = path.resolve(__dirname, '..');

test('the shipping component design pages validate and their examples compile', async t => {
  // The repository's Workbench space: these pages, with its preview settings.
  const compiler = await compilerFixture.open(root, null, { dir: path.resolve(root, '../..'), key: 'workbench' });
  t.after(() => compiler.close());
  const config = await compiler.config();
  assert.deepEqual(config.problems, []);
  const docs = Object.values(config.pages).map(page => page.docs).filter(Boolean);
  assert.ok(docs.length > 0);
  const base = 'http://127.0.0.1:' + compiler.port + '/_workbench/previews';
  for (const page of docs) {
    for (const lens of page.lenses) {
      const request = encodeURIComponent(JSON.stringify({ adapter: lens.adapter, examples: lens.examples, styles: lens.styles, ...(lens.environment ? { environment: lens.environment } : {}) }));
      const bundle = await fetch(base + '/docs/bundle?request=' + request);
      assert.equal(bundle.status, 200, page.file);
      const built = await bundle.json();
      assert.deepEqual(built.problems, [], page.file);
      assert.ok(built.examples.length > 0, page.file);
      const module = await (await fetch('http://127.0.0.1:' + compiler.port + built.module)).text();
      assert.equal(/const errors = (.*);/.exec(module)[1], '[]', page.file);
    }
  }
});

test('the Sidebar design loads shipping styles, scripts and interactive states in Chrome', {
  skip: !chrome.findChrome() && 'Chrome is required for the Sidebar design check',
}, async t => {
  const running = await server.start({ root, capture: { close() {} } });
  t.after(() => running.close());
  const browser = new chrome.Browser();
  await browser.launch();
  t.after(() => browser.close());
  const target = await browser.createTarget();
  await target.send('Log.enable');
  target.on('Log.entryAdded', event => {
    if (event.entry.level === 'error') t.diagnostic(event.entry.text);
  });
  const base = new URL(running.url).origin;
  for (const state of ['default', 'menu', 'filtered']) {
    await target.navigate(base + '/design/sidebar.html?state=' + state);
    const result = await target.evaluate(`(async () => {
      await Promise.race([
        customElements.whenDefined('wb-space-switcher'),
        new Promise((_, reject) => setTimeout(() => reject(new Error('Space switcher did not register')), 2000)),
      ]);
      const switcher = document.getElementById('spaces');
      const rows = document.querySelectorAll('#pageList button');
      function iconsIn(root) {
        const icons = [...root.querySelectorAll('svg.wb-icon')];
        for (const element of root.querySelectorAll('*'))
          if (element.shadowRoot) icons.push(...iconsIn(element.shadowRoot));
        return icons;
      }
      const icons = iconsIn(document);
      const iconBytes = performance.getEntriesByType('resource')
        .filter(entry => new URL(entry.name).pathname.includes('/node_modules/lucide/'))
        .reduce((sum, entry) => sum + entry.decodedBodySize, 0);
      return {
        background: getComputedStyle(document.body).backgroundColor,
        listReady: !!window.wbPageList && rows.length > 0,
        iconsReady: icons.length > 10 && icons.every(icon => icon.childElementCount > 0),
        lightweightIcons: iconBytes > 0 && iconBytes < 20000,
        current: switcher.currentId,
        open: switcher.open,
        search: document.getElementById('search').value,
        styles: Array.from(document.querySelectorAll('link[rel=stylesheet]')).every(link => !!link.sheet),
      };
    })()`);
    assert.deepEqual(result, {
      background: 'rgb(31, 31, 31)', listReady: true, iconsReady: true, lightweightIcons: true,
      current: 'acme', open: state === 'menu', search: state === 'filtered' ? 'sign' : '', styles: true,
    }, state);
  }
});
