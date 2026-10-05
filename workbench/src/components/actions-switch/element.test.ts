import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { createBrowserModules } from '../../server/browser-modules.ts';

const require = createRequire(import.meta.url);
interface Target { navigate(url: string): Promise<void>; evaluate(expression: string): Promise<unknown> }
const chrome = require('../../../scripts/chrome.cjs') as {
  findChrome(): string | null;
  Browser: new () => { launch(): Promise<void>; close(): Promise<void>; createTarget(): Promise<Target> };
};

test('Actions switch in real Chrome', {
  skip: !chrome.findChrome() && 'Chrome is required for Web Component interaction checks',
}, async t => {
  const browser = new chrome.Browser();
  t.after(() => browser.close());
  await browser.launch();
  const engine = require('../../../preview/engine.cjs') as typeof import('esbuild');
  const serve = createBrowserModules(path.resolve(fileURLToPath(new URL('../../', import.meta.url))),
    async (code, file) => (await engine.transform(code, { loader: 'ts', sourcefile: file, format: 'esm' })).code);
  const server = createServer(async (req, res) => {
    if (req.url?.startsWith('/_workbench/src/')) {
      const asset = await serve(req.url);
      res.writeHead(asset.status, { 'Content-Type': asset.type }); res.end(asset.body); return;
    }
    res.setHeader('Content-Type', 'text/html');
    res.setHeader('Content-Security-Policy', "default-src 'none'; script-src 'self' 'nonce-test'; style-src 'self'; connect-src 'self'");
    res.end(`<!doctype html><link rel="stylesheet" href="/_workbench/src/theme/defaults.css">
      <wb-actions-switch id="early"></wb-actions-switch>
      <script nonce="test">window.violations=[]; document.addEventListener('securitypolicyviolation', e => window.violations.push(e.violatedDirective));
        const early=document.getElementById('early'); early.checked=true; early.hint='set before upgrade';
        early.iconRenderer=(name)=>{const s=document.createElement('span');s.dataset.icon=name;return s;};</script>
      <script type="module" src="/_workbench/src/components/canvas-bootstrap.ts"></script>`);
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise<void>(resolve => server.close(() => resolve())));
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

    button.click();
    check(events.length === 1 && events[0][0].checked === false && events[0][1], 'asks for the other state, composed');
    check(el.checked, 'the controller owns the state');
    el.checked = false; check(button.getAttribute('aria-checked') === 'false' && events.length === 1, 'controller update is silent');
    el.hint = ''; check(button.title === 'Actions — let links navigate and forms submit', 'default hint');

    button.focus(); check(el.shadowRoot.activeElement === button, 'focusable');
    el.disabled = true; button.click();
    check(button.disabled && events.length === 1, 'locked asks nothing');
    el.disabled = false;

    el.remove(); document.body.append(el); button.click();
    check(events.length === 2 && events[1][0].checked === true, 'reconnect leaves one listener');

    const copy = document.createElement('wb-actions-switch'); document.body.append(copy);
    copy.checked = true; check(!el.checked && !copy.shadowRoot.querySelector('.icon').childNodes.length, 'independent instances; no icon without a renderer');
    check(getComputedStyle(copy.shadowRoot.querySelector('.track')).backgroundColor === 'rgb(0, 161, 255)', 'on uses the accent default');
    check(window.violations.length === 0, 'CSP permits adopted styles');
    return results.length;
  })()`);
  assert.equal(results, 14);
});
