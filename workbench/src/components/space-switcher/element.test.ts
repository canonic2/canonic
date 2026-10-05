import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { webviewComponents } from '../../server/webview-components.ts';
import { createBrowserModules } from '../../server/browser-modules.ts';

const require = createRequire(import.meta.url);
interface Target {
  navigate(url: string): Promise<void>;
  evaluate(expression: string): Promise<unknown>;
  send(method: string, params: object): Promise<unknown>;
}
const chrome = require('../../../scripts/chrome.cjs') as {
  findChrome(): string | null;
  Browser: new () => { launch(): Promise<void>; close(): Promise<void>; createTarget(): Promise<Target> };
};

test('space components in real Chrome: source modules and nonce webview bootstrap', {
  skip: !chrome.findChrome() && 'Chrome is required for Web Component interaction checks',
}, async t => {
  const browser = new chrome.Browser();
  t.after(() => browser.close());
  await browser.launch();
  const bundle = webviewComponents();
  const engine = require('../../../preview/engine.cjs') as typeof import('esbuild');
  const serve = createBrowserModules(path.resolve(fileURLToPath(new URL('../../', import.meta.url))),
    async (code, file) => (await engine.transform(code, { loader: 'ts', sourcefile: file, format: 'esm' })).code);
  const server = createServer(async (req, res) => {
    if (req.url?.startsWith('/_workbench/src/')) {
      const asset = await serve(req.url);
      res.writeHead(asset.status, { 'Content-Type': asset.type }); res.end(asset.body); return;
    }
    res.setHeader('Content-Type', 'text/html');
    // Exercise the same no-inline-style and nonce script constraints as sidebar-view.
    res.setHeader('Content-Security-Policy', "default-src 'none'; script-src 'self' 'nonce-test'; style-src 'self'; img-src 'self' data:; connect-src 'self'");
    res.end(`<!doctype html><link rel="stylesheet" href="/_workbench/src/theme/defaults.css">
      <wb-space-switcher id="early"></wb-space-switcher><button id="after">After</button>
      <script nonce="test">window.violations=[]; window.errors=[]; window.addEventListener('error', e => window.errors.push(e.message)); document.addEventListener('securitypolicyviolation', e => window.violations.push(e.violatedDirective));
        const early=document.getElementById('early'); early.spaces=[{id:'a',name:'Acme',initial:'A',color:'green'},{id:'b',name:'Beta',initial:'B',removable:true}]; early.currentId='a'; early.allowAdd=true; early.allowRemove=true;</script>
      ${req.url === '/source' ? '<script type="module" src="/_workbench/src/components/bootstrap.ts"></script>' : `<script nonce="test">${bundle}</script>`}`);
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise<void>(resolve => server.close(() => resolve())));
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  for (const host of ['source', 'webview']) {
    const target = await browser.createTarget();
    await target.navigate(`http://127.0.0.1:${address.port}/${host}`);
    assert.equal(await target.evaluate(String.raw`(async () => {
      await Promise.race([customElements.whenDefined('wb-space-switcher'), new Promise((_, reject) => setTimeout(() => reject(new Error('Registration failed: ' + JSON.stringify({errors:window.errors,violations:window.violations}))), 2000))]);
      const el = document.getElementById('early'), root = el.shadowRoot;
      const trigger = root.querySelector('.trigger'), menu = root.querySelector('.menu');
      const results = [];
      const check = (value, label) => { if (!value) throw new Error(label); results.push(label); };
      const key = (node, value) => node.dispatchEvent(new KeyboardEvent('keydown', {key:value,bubbles:true,composed:true,cancelable:true}));
      check(trigger.textContent.includes('Acme') && el.currentId === 'a' && el.allowAdd && el.allowRemove, 'pre-upgrade inputs');
      check(trigger.getAttribute('aria-haspopup') === 'menu' && menu.getAttribute('role') === 'menu', 'menu semantics');
      const events = [];
      document.addEventListener('wb-space-pick', e => events.push(['pick', e.detail.id, e.composed, e.bubbles]));
      el.addEventListener('wb-space-remove', e => events.push(['remove', e.detail.id]));
      el.addEventListener('wb-space-add', () => events.push(['add']));
      el.addEventListener('wb-space-toggle', e => events.push(['toggle', e.detail.open]));
      el.open = true; check(events.length === 0, 'programmatic updates are silent');
      el.open = false; trigger.focus(); key(trigger,'ArrowDown');
      check(root.activeElement.dataset.id === 'a' && el.open, 'keyboard opens selected row');
      key(root.activeElement,'ArrowDown'); check(root.activeElement.dataset.id === 'b', 'arrow navigation');
      const focused = root.activeElement;
      el.spaces = el.spaces.map(s => ({...s, name:s.name + '!'}));
      check(root.activeElement === focused && focused.textContent.includes('Beta!'), 'update preserves focus');
      focused.click(); check(el.currentId === 'a' && root.activeElement === trigger && !el.open, 'pick remains controller owned');
      check(events.some(e => e[0] === 'pick' && e[1] === 'b' && e[2] && e[3]), 'composed pick event');
      trigger.click(); key(root.activeElement,'End'); check(root.activeElement.dataset.action === 'add', 'End reaches add');
      root.activeElement.click(); check(events.some(e => e[0] === 'add'), 'add intent');
      trigger.click(); key(root.activeElement,'Home'); key(root.activeElement,'ArrowUp'); check(root.activeElement.dataset.action === 'add', 'arrow wraps');
      key(root.activeElement,'ArrowUp'); check(root.activeElement.dataset.action === 'remove', 'remove is keyboard reachable');
      root.activeElement.click(); check(events.some(e => e[0] === 'remove' && e[1] === 'b'), 'remove intent');
      trigger.click(); key(root.activeElement,'Escape'); check(!el.open && root.activeElement === trigger, 'Escape restores trigger');
      trigger.click(); key(root.activeElement,'Tab'); check(!el.open, 'Tab closes menu');
      trigger.click(); document.getElementById('after').dispatchEvent(new PointerEvent('pointerdown',{bubbles:true,composed:true})); check(!el.open, 'outside press closes');
      const copy = document.createElement('wb-space-switcher'); copy.spaces=el.spaces; copy.currentId='b'; document.body.append(copy);
      el.open=true; check(!copy.open && copy.currentId==='b', 'independent instances');
      copy.open=true;
      check(getComputedStyle(copy.shadowRoot.querySelector('.add')).display==='none', 'unavailable add action is hidden');
      copy.open=false;
      el.disabled=true; check(!el.open && trigger.disabled, 'disabled closes'); el.disabled=false;
      el.open=true; el.remove(); check(!el.open, 'disconnect closes');
      const count=events.length; trigger.click(); window.dispatchEvent(new Event('blur')); check(!el.open && events.length===count, 'disconnected handlers released');
      document.body.prepend(el); trigger.click(); check(el.open, 'reconnect works');
      check(events.filter(e => e[0]==='toggle').slice(-1)[0][1]===true, 'reconnect emits once');
      key(root.activeElement,'Home'); el.spaces=[{id:'b',name:'Beta',removable:true}];
      check(root.activeElement.dataset.id==='b', 'removed focused row gets fallback');
      el.open=false;
      const palette=getComputedStyle(trigger).backgroundColor;
      check(palette==='rgb(44, 44, 44)', 'standalone default');
      el.style.setProperty('--wb-raised','#fafafa'); el.style.setProperty('--wb-fg','#101010');
      el.style.setProperty('--wb-focus','#ff00ff');
      check(getComputedStyle(trigger).backgroundColor==='rgb(250, 250, 250)' && getComputedStyle(trigger).color==='rgb(16, 16, 16)', 'host tokens cross shadow DOM');
      const mark=root.querySelector('wb-space-mark'); check(mark.shadowRoot, 'nested shadow root');
      check(window.violations.length===0, 'webview CSP permits adopted styles');
      copy.remove(); el.remove();
      return results.length;
    })()`), 28, host);
    // Test actual Tab traversal, not just a synthetic keydown.
    await target.evaluate(`document.getElementById('after').before(document.createElement('wb-space-switcher'));`);
    await target.evaluate(`document.querySelector('wb-space-switcher').shadowRoot.querySelector('button').focus()`);
    await target.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 });
    await target.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 });
    assert.equal(await target.evaluate(`document.activeElement.id`), 'after');
    await target.evaluate(`document.querySelector('wb-space-switcher').allowAdd=true; document.querySelector('wb-space-switcher').shadowRoot.querySelector('button').click()`);
    await target.send('Input.dispatchKeyEvent', { type: 'keyDown', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 });
    await target.send('Input.dispatchKeyEvent', { type: 'keyUp', key: 'Tab', code: 'Tab', windowsVirtualKeyCode: 9 });
    assert.equal(await target.evaluate(`document.activeElement.id`), 'after', 'Tab from the open menu leaves the widget');
  }
});
