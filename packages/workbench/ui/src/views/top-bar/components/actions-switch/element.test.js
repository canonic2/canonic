import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { createUiAssets } from '../../../../../../scripts/ui-browser-fixture.cjs';

const require = createRequire(import.meta.url);

const chrome = require('../../../../../../scripts/chrome.cjs');

void test(
  'Actions switch in real Chrome',
  {
    skip: !chrome.findChrome() && 'Chrome is required for Web Component interaction checks',
  },
  async (t) => {
    const browser = new chrome.Browser();
    t.after(() => browser.close());
    await browser.launch();
    const serve = createUiAssets(
      path.resolve(fileURLToPath(new URL('../../../../../..', import.meta.url))),
    );
    const server = createServer((req, res) => {
      void (async () => {
        if (req.url?.startsWith('/_workbench/')) {
          const asset = await serve(req.url);
          res.writeHead(asset.status, { 'Content-Type': asset.type });
          res.end(asset.body);
          return;
        }
        res.setHeader('Content-Type', 'text/html');
        res.setHeader(
          'Content-Security-Policy',
          "default-src 'none'; script-src 'self' 'nonce-test'; style-src 'self'; connect-src 'self'",
        );
        res.end(`<!doctype html><script type="importmap" nonce="test">{"imports":{"@canonic2/lattice":"/_workbench/node_modules/@canonic2/lattice/src/index.js","@canonic2/lattice/elements":"/_workbench/node_modules/@canonic2/lattice/src/modules/elements/elements.js"}}</script><link rel="stylesheet" href="/_workbench/ui/src/theme/defaults.css">
      <wb-actions-switch id="early"></wb-actions-switch>
      <script nonce="test">window.violations=[]; document.addEventListener('securitypolicyviolation', e => window.violations.push(e.violatedDirective));
        const early=document.getElementById('early'); early.checked=true; early.hint='set before upgrade';
        early.iconRenderer=(name)=>{const s=document.createElement('span');s.dataset.icon=name;return s;};</script>
      <script type="module" src="/_workbench/ui/src/app/workbench/workbench.js"></script>`);
      })().catch((error) => {
        res.statusCode = 500;
        res.end(String(error));
      });
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    t.after(() => new Promise((resolve) => server.close(() => resolve())));
    const address = server.address();
    assert.ok(address && typeof address !== 'string');
    const target = await browser.createTarget();
    await target.navigate(`http://127.0.0.1:${address.port}/`);
    const results = await target.evaluate(String.raw`(async () => {
    await Promise.race([customElements.whenDefined('wb-actions-switch'),
      new Promise((_, reject) => setTimeout(() => reject(new Error('Registration failed: ' + JSON.stringify(window.violations))), 3000))]);
    const results = [];
    const check = (value, label) => { if (!value) throw new Error(label); results.push(label); };
    const events = [];
    document.addEventListener('wb-actions-toggle', e => events.push([e.detail, e.composed && e.bubbles]));

    const el = document.getElementById('early'), button = el.shadowRoot.querySelector('button');
    check(el.checked && el.hasAttribute('checked') && button.getAttribute('aria-checked') === 'true', 'input set before upgrade is kept and reflected');
    check(button.title === 'Actions — set before upgrade' && el.shadowRoot.querySelector('[data-icon=mouse-pointer-click]'), 'hint and icon from early inputs');
    check(button.getAttribute('role') === 'switch' && button.getAttribute('aria-label') === 'Actions', 'a named switch');
    check(events.length === 0, 'inputs emit nothing');
    const glyph = el.shadowRoot.querySelector('.icon').firstChild;

    button.click();
    check(events.length === 1 && events[0][0].checked === false && events[0][1], 'asks for the other state, composed');
    check(el.checked, 'the controller owns the state');
    el.checked = false; check(button.getAttribute('aria-checked') === 'false' && events.length === 1, 'controller update is silent');
    el.hint = ''; check(button.title === 'Actions — let links navigate and forms submit', 'default hint');

    button.focus(); check(el.shadowRoot.activeElement === button, 'focusable');
    el.disabled = true; button.click();
    check(button.disabled && events.length === 1, 'locked asks nothing');
    el.disabled = false;
    check(el.shadowRoot.querySelector('.icon').firstChild === glyph, 'state and hint changes retain the icon');

    el.remove(); document.body.append(el); button.click();
    check(events.length === 2 && events[1][0].checked === true, 'reconnect leaves one listener');

    const copy = document.createElement('wb-actions-switch'); document.body.append(copy);
    check(copy.shadowRoot.adoptedStyleSheets[0] === el.shadowRoot.adoptedStyleSheets[0], 'instances share their parsed stylesheet');
    copy.checked = true; check(!el.checked && !copy.shadowRoot.querySelector('.icon').childNodes.length, 'independent instances; no icon without a renderer');
    check(getComputedStyle(copy.shadowRoot.querySelector('.track')).backgroundColor === 'rgb(0, 161, 255)', 'on uses the accent default');
    check(window.violations.length === 0, 'CSP permits adopted styles');
    return results.length;
  })()`);
    assert.equal(results, 16);
  },
);
