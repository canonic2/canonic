import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { createBrowserModules } from '../../server/browser-modules.ts';
const require = createRequire(import.meta.url);
const chrome = require('../../../scripts/chrome.cjs');
test('export dialog selects scope and defaults visual pages to the current view', { skip: !chrome.findChrome() }, async t => {
  const browser = new chrome.Browser(); t.after(() => browser.close()); await browser.launch();
  const engine = require('../../../preview/engine.cjs');
  const serve = createBrowserModules(path.resolve(fileURLToPath(new URL('../../', import.meta.url))),
    async (code, file) => (await engine.transform(code, { loader: 'ts', sourcefile: file, format: 'esm' })).code);
  const server = createServer(async (req, res) => {
    if (req.url?.startsWith('/_workbench/src/')) { const asset = await serve(req.url); res.writeHead(asset.status, { 'Content-Type': asset.type }); res.end(asset.body); return; }
    res.setHeader('Content-Type', 'text/html');
    res.end('<!doctype html><button id="opener">Export</button><wb-export-dialog></wb-export-dialog><script>document.querySelector("wb-export-dialog").pages=[{id:"early",label:"Early"}]</script><script type="module" src="/_workbench/src/components/export-dialog/element.ts"></script>');
  });
  await new Promise<void>(resolve => server.listen(0, '127.0.0.1', resolve));
  t.after(() => new Promise<void>(resolve => server.close(() => resolve())));
  const address = server.address(); assert.ok(address && typeof address !== 'string');
  const target = await browser.createTarget(); await target.navigate(`http://127.0.0.1:${address.port}/`);
  const result = await target.evaluate(`(async()=>{
    await Promise.race([customElements.whenDefined('wb-export-dialog'), new Promise((_, reject)=>setTimeout(()=>reject(new Error('Dialog registration failed')),3000))]);
    const el=document.querySelector('wb-export-dialog'), events=[];
    if(el.pages[0].id!=='early') throw new Error('Input before registration was lost');
    el.pages=[{id:'a',label:'A'},{id:'b',label:'B'}]; el.collections=['Pages']; el.current={page:'a',width:777,height:600};
    el.addEventListener('wb-export-request',e=>events.push(e.detail));
    document.getElementById('opener').focus(); el.show();
    const root=el.shadowRoot, field=n=>root.querySelector('[name='+n+']');
    const sourceDefault=field('variants').value;
    field('format').value='pdf';field('format').dispatchEvent(new Event('change',{bubbles:true}));
    const visualDefault=field('variants').value;
    root.querySelector('form').requestSubmit();
    el.show();field('scope').value='pages';field('scope').dispatchEvent(new Event('change',{bubbles:true}));
    field('pages').options[1].selected=true;root.querySelector('form').requestSubmit();
    el.show();root.querySelector('button[type=button]').click();
    return {sourceDefault,visualDefault,events,closed:!root.querySelector('dialog').open};
  })()`);
  assert.equal(result.sourceDefault, 'all'); assert.equal(result.visualDefault, 'current');
  assert.equal(result.events[0].format, 'pdf'); assert.equal(result.events[0].current.width, 777);
  assert.deepEqual(result.events[1].pages, ['b']); assert.equal(result.events.length, 2); assert.ok(result.closed);
});
