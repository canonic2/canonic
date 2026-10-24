import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { createRequire } from 'node:module';

const require = createRequire(import.meta.url);
const server = require('../../../server.js');
const chrome = require('../../../../../scripts/chrome.cjs');

test('a first Docs page has a laid-out content viewport before it reports ready', {
  skip: !chrome.findChrome() && 'Chrome is required for Docs startup checks',
}, async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-docs-start-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.writeFileSync(path.join(root, 'workbench.yaml'), `name: Acme
previews: false
collections:
  - name: Docs
    items:
      - label: Overview
        src: overview.md
`);
  fs.writeFileSync(path.join(root, 'overview.md'), '# Overview\n\nThe first documentation page.\n');
  const running = await server.start({ root, capture: { close() {} } });
  t.after(() => running.close());
  const browser = new chrome.Browser();
  t.after(() => browser.close());
  await browser.launch();
  const target = await browser.createTarget();
  const errors: string[] = [];
  target.on('Runtime.exceptionThrown', (event: { exceptionDetails: { text: string } }) => errors.push(event.exceptionDetails.text));
  await target.navigate(running.url);
  const ready = `document.getElementById('artboards')?.state.artboards[0]?.view?.ready`;
  const deadline = Date.now() + 8000;
  while (!await target.evaluate(ready)) {
    if (Date.now() > deadline) {
      const view = await target.evaluate(`({state:document.getElementById('artboards')?.state,blank:document.getElementById('blank')?.textContent,frames:[...document.querySelectorAll('wb-canvas > iframe')].map(f=>({width:f.clientWidth,height:f.clientHeight}))})`);
      assert.fail('Docs never became ready: ' + JSON.stringify({ view, errors }));
    }
    await new Promise(resolve => setTimeout(resolve, 20));
  }
  assert.equal(await target.evaluate(`document.getElementById('artboards').reading`), true);
  assert.equal(await target.evaluate(`document.getElementById('blank').hidden`), true);
  assert.equal(await target.evaluate(`document.querySelector('wb-canvas > iframe').clientWidth > 0`), true);
  assert.deepEqual(errors, []);
});

test('page picks retain the outgoing artboard for the grace period and commit only the latest ready page', {
  skip: !chrome.findChrome() && 'Chrome is required for navigation presentation checks',
}, async t => {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-navigation-'));
  t.after(() => fs.rmSync(root, { recursive: true, force: true }));
  fs.writeFileSync(path.join(root, 'workbench.yaml'), `name: Acme
previews: false
sizes:
  narrow:
    width: 340
    height: 400
  wide:
    width: 800
    height: 600
collections:
  - name: Pages
    items:
      - label: One
        src: one.html
        sizes:
          - narrow
      - label: Two
        src: two.html
        sizes:
          - wide
      - label: Three
        src: three.html
      - label: Broken
        src: broken.html
`);
  fs.writeFileSync(path.join(root, 'one.html'), '<!doctype html><title>One</title><p>One</p>');
  fs.writeFileSync(path.join(root, 'broken.html'), '<!doctype html><script>window.__workbenchError="Broken page";</script>');
  for (const name of ['two', 'three']) fs.writeFileSync(path.join(root, name + '.html'), `<!doctype html><title>${name}</title><p>${name}</p><script>
    window.__workbenchOptions={};window.__workbenchReady=false;
    window.release=()=>{window.__workbenchReady=true;dispatchEvent(new Event('workbench:ready'));};
  </script>`);
  const running = await server.start({ root, capture: { close() {} } });
  t.after(() => running.close());
  const browser = new chrome.Browser();
  t.after(() => browser.close());
  await browser.launch();
  const target = await browser.createTarget();
  const errors: string[] = [];
  target.on('Runtime.exceptionThrown', (event: { exceptionDetails: { exception?: { description?: string }; text: string } }) =>
    errors.push(event.exceptionDetails.exception?.description || event.exceptionDetails.text));
  async function until(expression: string) {
    const deadline = Date.now() + 8000;
    while (!await target.evaluate(expression)) {
      if (Date.now() > deadline) throw new Error(expression + '\n' + errors.join('\n') + '\n' + await target.evaluate(`JSON.stringify({state:document.getElementById('artboards')?.state,toast:document.getElementById('toast')?.textContent,frames:[...document.querySelectorAll('wb-canvas > iframe')].map(f=>({style:f.style.cssText,width:f.clientWidth,surface:f.contentDocument.getElementById('artboard')?.style.cssText,frames:[...f.contentDocument.querySelectorAll('#artboardContent iframe')].map(child=>({src:child.src,style:child.style.cssText,width:child.clientWidth}))}))})`));
      await new Promise(resolve => setTimeout(resolve, 20));
    }
  }
  const state = `document.getElementById('artboards').state`;
  await target.navigate(running.url);
  await until(`${state}.artboards[0]?.view?.ready && ${state}.artboards[0].target.src==='one.html'`);
  await target.evaluate(`(() => {
    // Control only the documented grace deadline; document loads, DOM, frames
    // and readiness signals still use the real host and browser.
    const timeout=window.setTimeout.bind(window), cancel=window.clearTimeout.bind(window);
    window.grace=new Map();let id=0;
    window.setTimeout=(callback,ms,...args)=>{if(ms!==200)return timeout(callback,ms,...args);const key=--id;grace.set(key,()=>callback(...args));return key;};
    window.clearTimeout=key=>{if(key<0)grace.delete(key);else cancel(key);};
    window.expire=()=>{const callbacks=[...grace.values()];grace.clear();callbacks.forEach(callback=>callback());};
    window.preview=src=>[...document.querySelectorAll('wb-canvas > iframe')]
      .flatMap(frame=>[...frame.contentDocument.querySelectorAll('#artboardContent iframe')])
      .find(frame=>new URL(frame.src||'about:blank').pathname.endsWith('/'+src));
    window.pick=src=>document.querySelector('#pageList button[data-src="'+src+'"]').click();
    window.snapshot=()=>{
      const canvas=document.getElementById('artboards');
      return {src:canvas.state.artboards[0]?.target.src,visible:getComputedStyle(canvas).visibility,
        label:canvas.shadowRoot.querySelector('section button')?.textContent,
        loader:!document.getElementById('blank').hidden,inert:canvas.inert};
    };
    window.wbReference.copy=text=>{window.reference=text;return Promise.resolve();};
  })()`);
  await target.evaluate(`document.getElementById('artboards').zoomTo(0.5)`);
  await target.evaluate(`pick('two.html')`);
  await until(`!!preview('two.html')?.contentWindow.release`);
  let shown = await target.evaluate('snapshot()');
  assert.equal(shown.src, 'one.html');
  assert.equal(shown.visible, 'visible');
  assert.match(shown.label, /One/);
  assert.equal(shown.loader, false);
  assert.equal(shown.inert, true, 'outgoing pixels are display-only while navigation is pending');
  assert.equal(await target.evaluate(`document.getElementById('artboards').scale`), 0.5, 'automatic fitting waits for the replacement');
  assert.equal(await target.evaluate(`document.querySelector('#pageList button[data-src="two.html"]').getAttribute('aria-current')`), 'true');
  assert.equal(await target.evaluate(`getComputedStyle(preview('one.html')).visibility`), 'visible');
  assert.equal(await target.evaluate(`preview('one.html').contentWindow.innerWidth`), 340, 'pending destination sizing does not reflow the outgoing page');
  await target.evaluate(`document.getElementById('copyReference').click()`);
  assert.match(await target.evaluate('reference'), /one\.html/);
  assert.doesNotMatch(await target.evaluate('reference'), /two\.html/);
  await target.evaluate(`preview('two.html').contentWindow.release()`);
  await until(`${state}.artboards[0]?.target.src==='two.html' && !document.getElementById('artboards').inert`);
  assert.equal(await target.evaluate('grace.size'), 0, 'a fast ready page cancels its deadline');
  assert.equal((await target.evaluate('snapshot()')).loader, false);
  await until(`preview('two.html').contentWindow.innerWidth===800`);

  await target.evaluate(`pick('three.html')`);
  await until(`!!preview('three.html')?.contentWindow.release`);
  assert.equal((await target.evaluate('snapshot()')).src, 'two.html');
  await target.evaluate('expire()');
  shown = await target.evaluate('snapshot()');
  assert.equal(shown.visible, 'hidden');
  assert.equal(shown.loader, true);
  await target.evaluate(`document.getElementById('copyReference').click()`);
  assert.match(await target.evaluate('reference'), /0 artboard/);
  await target.evaluate(`pick('two.html')`);
  await until(`!document.getElementById('artboards').inert && ${state}.artboards[0]?.target.src==='two.html'`);
  await target.evaluate(`preview('three.html')?.contentWindow.release()`);
  assert.equal((await target.evaluate('snapshot()')).src, 'two.html', 'late readiness cannot replace the latest destination');
  assert.equal(await target.evaluate('grace.size'), 0);
  await target.evaluate(`pick('broken.html')`);
  await until(`${state}.artboards[0]?.view?.problem==='Broken page'`);
  assert.match((await target.evaluate('snapshot()')).label, /Broken page/);
  assert.equal(await target.evaluate(`getComputedStyle(preview('two.html')).visibility`), 'hidden', 'failure cannot relabel the outgoing pixels as the broken page');
  await target.evaluate(`pick('two.html')`);
  await until(`!document.getElementById('artboards').inert && ${state}.artboards[0]?.target.src==='two.html'`);
  assert.deepEqual(errors, []);
});
