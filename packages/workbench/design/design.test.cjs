const { test } = require('node:test');
const assert = require('node:assert/strict');
const path = require('node:path');
const { Compiler, docsRequest } = require('../server/node/preview/compiler.cjs');
const server = require('../server/node/server.js');
const chrome = require('../scripts/chrome.cjs');

const root = path.resolve(__dirname, '..');

test('the shipping component design pages validate and their examples compile', async () => {
  const compiler = new Compiler(root, { include: ['design/**/*.workbench.ts'] });
  const index = await compiler.index();
  assert.deepEqual(index.errors, []);
  assert.ok(index.docs.length > 0);
  for (const page of index.docs) {
    for (const lens of page.lenses) {
      const listed = await compiler.docsExamples(docsRequest(lens, root));
      assert.deepEqual(listed.problems, [], page.file);
      assert.ok(listed.examples.length > 0, page.file);
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
  const base = new URL(running.url).origin;
  for (const state of ['default', 'menu', 'filtered']) {
    await target.navigate(base + '/design/sidebar.html?state=' + state);
    const result = await target.evaluate(`(async () => {
      await customElements.whenDefined('wb-space-switcher');
      const switcher = document.getElementById('spaces');
      const rows = document.querySelectorAll('#pageList button');
      return {
        background: getComputedStyle(document.body).backgroundColor,
        listReady: !!window.wbPageList && rows.length > 0,
        iconsReady: !!window.wbIcon && !!document.querySelector('[data-icon] svg'),
        current: switcher.currentId,
        open: switcher.open,
        search: document.getElementById('search').value,
        styles: Array.from(document.querySelectorAll('link[rel=stylesheet]')).every(link => !!link.sheet),
      };
    })()`);
    assert.deepEqual(result, {
      background: 'rgb(31, 31, 31)', listReady: true, iconsReady: true,
      current: 'acme', open: state === 'menu', search: state === 'filtered' ? 'sign' : '', styles: true,
    }, state);
  }
});
