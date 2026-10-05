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

const SIZES = JSON.stringify([
  { key: 'fit', label: 'Fit', icon: 'minimize-2', button: true, kind: 'fit', width: null, height: null },
  { key: 'laptop', label: 'Laptop', icon: 'monitor', button: true, kind: 'fixed', width: 1512, height: 982 },
  { key: 'sidebar', label: 'Sidebar', icon: 'panel-left', button: true, kind: 'fixed', width: 340, height: 'fill' },
  { key: 'tablet', label: 'Tablet', icon: 'tablet', button: false, kind: 'fixed', width: 1024, height: 1366 },
  { key: 'phone', label: 'Phone', icon: 'smartphone', button: false, kind: 'fixed', width: 375, height: 667, local: true },
]);

test('size switcher, Custom size… and Edit sizes… in real Chrome', {
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
      <button id="before">Before</button><wb-size-switcher id="switcher"></wb-size-switcher><button id="after">After</button>
      <wb-size-dialog id="dialog"></wb-size-dialog><wb-sizes-editor id="editor"></wb-sizes-editor>
      <script nonce="test">window.violations=[]; document.addEventListener('securitypolicyviolation', e => window.violations.push(e.violatedDirective));
        const early=document.getElementById('switcher'); early.sizes=${SIZES}; early.current='sidebar'; early.supported=['fit','sidebar','tablet'];
        early.iconRenderer=(name)=>{const s=document.createElement('span');s.dataset.icon=name;s.textContent=name.charAt(0);return s;};</script>
      <script type="module" src="/_workbench/src/components/canvas-bootstrap.ts"></script>`);
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise<void>(resolve => server.close(() => resolve())));
  const address = server.address();
  assert.ok(address && typeof address !== 'string');
  const target = await browser.createTarget();
  await target.navigate(`http://127.0.0.1:${address.port}/`);
  const results = await target.evaluate(String.raw`(async () => {
    await Promise.race([Promise.all(['wb-size-switcher','wb-size-dialog','wb-sizes-editor'].map(n => customElements.whenDefined(n))),
      new Promise((_, reject) => setTimeout(() => reject(new Error('Registration failed: ' + JSON.stringify(window.violations))), 3000))]);
    const results = [];
    const check = (value, label) => { if (!value) throw new Error(label); results.push(label); };
    const key = (node, value) => node.dispatchEvent(new KeyboardEvent('keydown', {key:value,bubbles:true,composed:true,cancelable:true}));
    const events = [];
    for (const name of ['wb-size-pick','wb-size-custom','wb-size-edit','wb-size-toggle','wb-size-add','wb-size-dialog-close','wb-sizes-save','wb-sizes-close'])
      document.addEventListener(name, e => events.push([name, e.detail, e.composed && e.bubbles]));

    /* The switcher */
    const el = document.getElementById('switcher'), root = el.shadowRoot;
    const buttons = [...root.querySelectorAll('.size')], more = root.querySelector('.more'), menu = root.querySelector('.menu');
    check(buttons.map(b => b.dataset.key).join() === 'fit,laptop,sidebar', 'buttons are the space’s button sizes, in order');
    check(root.querySelector('.group').getAttribute('role') === 'group' && root.querySelector('.group').getAttribute('aria-label') === 'Size', 'a named group');
    check(buttons[2].getAttribute('aria-pressed') === 'true' && buttons[0].getAttribute('aria-pressed') === 'false', 'current size pressed');
    check(buttons[1].disabled && buttons[1].title === 'Laptop, 1512 × 982 · not supported by this page', 'unsupported size disabled, with why');
    check(buttons[2].getAttribute('aria-label') === 'Sidebar, 340 × fill', 'buttons named with dimensions');
    check(more.getAttribute('aria-pressed') === 'false' && more.getAttribute('aria-label') === 'More sizes' && more.textContent === 'c', 'menu button is a chevron while a button size shows');
    check(menu.hidden && more.getAttribute('aria-haspopup') === 'menu', 'menu closed, with menu semantics');
    buttons[0].click(); check(events.at(-1)[0] === 'wb-size-pick' && events.at(-1)[1].key === 'fit' && events.at(-1)[2], 'button pick is a composed intent');
    check(el.current === 'sidebar', 'the controller owns the current size');
    const count = events.length; buttons[1].click(); buttons[2].click(); check(events.length === count, 'disabled and current sizes ask nothing');

    more.focus(); key(more, 'ArrowDown');
    check(el.open && root.activeElement.dataset.key === 'sidebar', 'keyboard opens on the checked size');
    const headings = [...menu.querySelectorAll('.heading')].map(h => h.textContent);
    check(headings.join() === 'Sizes,More sizes', 'Sizes then More sizes');
    const items = [...menu.querySelectorAll('[role=menuitemradio]')];
    check(items.map(i => i.dataset.key).join() === 'fit,laptop,sidebar,tablet,phone', 'every size listed');
    check(items[2].querySelector('.dims').textContent === '340 × fill' && items[0].querySelector('.dims').textContent === '', 'dimensions beside each size');
    check(items[1].disabled && items[4].disabled && !items[3].disabled, 'menu disables what the page doesn’t support');
    check(!menu.querySelector('[data-action=custom]'), 'no edit actions unless allowed');
    key(root.activeElement, 'ArrowDown'); check(root.activeElement.dataset.key === 'tablet', 'arrows skip disabled entries');
    key(root.activeElement, 'ArrowDown'); check(root.activeElement.dataset.key === 'fit', 'arrows wrap');
    key(root.activeElement, 'End'); root.activeElement.click();
    check(events.at(-2)[0] === 'wb-size-toggle' && events.at(-1)[0] === 'wb-size-pick' && events.at(-1)[1].key === 'tablet', 'picking from the menu');
    check(!el.open && root.activeElement === more, 'picking closes and returns focus');

    el.current = 'tablet';
    check(more.getAttribute('aria-pressed') === 'true' && more.getAttribute('aria-label') === 'Tablet, 1024 × 1366' && more.textContent === 't', 'menu button shows a size that isn’t a button');
    check(buttons.every(b => b.getAttribute('aria-pressed') === 'false'), 'no button pressed then');

    el.allowEdit = true; more.click(); key(more, 'ArrowDown');
    const actions = [...menu.querySelectorAll('[role=menuitem]')].map(i => i.textContent);
    check(actions.join() === 'Custom size…,Edit sizes…', 'actions after the sizes');
    menu.querySelector('[data-action=custom]').click(); check(events.at(-1)[0] === 'wb-size-custom' && !el.open, 'Custom size… asks');
    more.click(); menu.querySelector('[data-action=edit]').click(); check(events.at(-1)[0] === 'wb-size-edit', 'Edit sizes… asks');
    more.click(); key(root.activeElement || more, 'Escape'); check(!el.open && root.activeElement === more, 'Escape restores the menu button');
    more.click(); document.getElementById('after').dispatchEvent(new PointerEvent('pointerdown', {bubbles:true,composed:true})); check(!el.open, 'outside press closes');
    const silent = events.length; el.open = true; el.open = false; el.sizes = el.sizes; check(events.length === silent, 'programmatic updates are silent');

    el.supported = []; el.current = ''; el.setAttribute('unsupported-reason', 'a docs page fills the canvas');
    check(buttons.every(b => b.disabled && b.getAttribute('aria-pressed') === 'false'), 'a docs page disables every size');
    check(buttons[0].title.endsWith('· a docs page fills the canvas'), 'with the docs reason');
    more.click(); check(el.open && !menu.querySelector('[data-action=custom]').disabled, 'Custom size… still offered on a docs page');
    el.remove(); check(!el.open, 'disconnect closes');
    document.getElementById('before').after(el); more.click(); check(el.open, 'reconnect works');
    el.open = false;

    const copy = document.createElement('wb-size-switcher'); copy.sizes = el.sizes; copy.current = 'fit'; document.body.append(copy);
    el.open = true; check(!copy.open && copy.current === 'fit', 'independent instances'); el.open = false; copy.remove();

    /* Custom size… */
    const dialog = document.getElementById('dialog'), droot = dialog.shadowRoot;
    const native = droot.querySelector('dialog'), add = droot.querySelector('.primary');
    const field = name => droot.querySelector('[name=' + name + ']');
    const type = (name, value) => { const f = field(name); if (f.type === 'checkbox') f.checked = value; else f.value = value; f.dispatchEvent(new Event('input', {bubbles:true,composed:true})); };
    dialog.open = true;
    check(native.open && droot.activeElement === field('name'), 'opens modal, focus on Name');
    check(add.disabled && droot.querySelector('.problem').textContent === '', 'Add waits for a size, quietly');
    check(droot.querySelector('label.check:has([name=pageOnly])').hidden, 'Only for this page needs a listed page');
    type('name', 'Wide'); type('width', '0');
    check(add.disabled && /^Width must be/.test(droot.querySelector('.problem').textContent), 'says why Add is off');
    type('width', '1200'); type('heightFill', true);
    check(field('height').disabled && !add.disabled, 'Fill replaces the number');
    type('icon', 'panel-top'); check(droot.querySelector('.preview').textContent === '', 'icon previews without a renderer');
    add.click();
    const added = events.filter(e => e[0] === 'wb-size-add').at(-1);
    check(added && JSON.stringify(added[1]) === JSON.stringify({ name:'Wide', width:1200, height:'fill', icon:'panel-top', button:false, pageOnly:false }) && added[2], 'Add asks with the size');
    check(dialog.open && native.open, 'the controller closes it');
    dialog.pending = true; check(add.disabled && field('name').disabled && add.textContent === 'Adding…', 'pending');
    key(native, 'Escape'); native.dispatchEvent(new Event('cancel', {cancelable:true}));
    check(!events.some(e => e[0] === 'wb-size-dialog-close'), 'can’t be dismissed while pending');
    dialog.pending = false; dialog.error = 'Size “wide” exists.'; check(droot.querySelector('.problem').textContent === 'Size “wide” exists.', 'controller error shown');
    type('name', 'Wider'); check(droot.querySelector('.problem').textContent === '', 'typing clears it');
    dialog.allowPageOnly = true; type('button', true); type('pageOnly', true);
    check(field('button').disabled && !droot.querySelector('label.check:has([name=pageOnly])').hidden, 'a page’s own size is never a button');
    native.dispatchEvent(new Event('cancel', {cancelable:true})); check(events.at(-1)[0] === 'wb-size-dialog-close' && native.open, 'Escape asks to close');
    dialog.open = false; check(!native.open, 'closes'); dialog.open = true; check(field('name').value === '' && !field('pageOnly').checked, 'opens empty'); dialog.open = false;

    /* Edit sizes… */
    const editor = document.getElementById('editor'), eroot = editor.shadowRoot;
    editor.sizes = ${SIZES}; editor.open = true;
    const rows = () => [...eroot.querySelectorAll('li.size')];
    const save = eroot.querySelector('.primary');
    check(eroot.querySelector('dialog').open && rows().length === 5, 'opens with every size');
    check(rows()[0].querySelector('.none').textContent === 'Fills the canvas', 'Fit has no dimensions');
    check(rows()[4].querySelectorAll('input:not(:disabled)').length === 0 && rows()[4].querySelector('[data-action=remove]').disabled, 'local rows are read-only');
    check(rows()[0].querySelector('[data-action=up]').disabled && rows()[4].querySelector('[data-action=down]').disabled, 'ends can’t move further');
    rows()[1].querySelector('[data-action=down]').click();
    check(rows().map(r => r.dataset.key).join() === 'fit,sidebar,laptop,tablet,phone' && eroot.activeElement.dataset.action === 'down' && eroot.activeElement.closest('li').dataset.key === 'laptop', 'moving keeps focus on the row');
    rows()[3].querySelector('[data-action=remove]').click();
    check(rows().map(r => r.dataset.key).join() === 'fit,sidebar,laptop,phone', 'remove');
    const name = rows()[1].querySelector('[data-field=name]'); name.focus(); name.value = ''; name.dispatchEvent(new Event('input', {bubbles:true,composed:true}));
    check(save.disabled && eroot.querySelector('.problem').textContent === 'sidebar needs a name.', 'Save waits for valid rows');
    name.value = 'Panel'; name.dispatchEvent(new Event('input', {bubbles:true,composed:true}));
    check(eroot.activeElement === name && rows()[1].querySelector('[data-field=name]') === name, 'typing keeps the row and its focus');
    save.click();
    const saved = events.filter(e => e[0] === 'wb-sizes-save').at(-1)[1].sizes;
    check(JSON.stringify(saved) === JSON.stringify([{key:'fit',value:true},{key:'sidebar',value:{label:'Panel',width:340,height:'fill',icon:'panel-left',button:true}},{key:'laptop',value:true},{key:'phone',value:{width:375,height:667,icon:'smartphone'}}]), 'Save asks with every row as written');
    editor.sizes = [${'{'}key:'fit',label:'Fit',icon:'minimize-2',button:true,kind:'fit',width:null,height:null${'}'}];
    check(rows().length === 4, 'inputs don’t replace an open draft');
    editor.open = false; editor.open = true;
    check(rows().length === 1 && rows()[0].querySelector('[data-action=remove]').disabled, 'reopens from its sizes; the last size stays');
    eroot.querySelector('dialog').dispatchEvent(new Event('cancel', {cancelable:true})); check(events.at(-1)[0] === 'wb-sizes-close', 'Escape asks to close');
    editor.open = false;
    check(window.violations.length === 0, 'CSP permits adopted styles');
    return results.length;
  })()`);
  assert.equal(typeof results, 'number');
  assert.ok((results as number) > 50);
});
