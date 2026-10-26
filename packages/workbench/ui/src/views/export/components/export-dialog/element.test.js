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
  'export dialog selects scope and defaults visual pages to the current view',
  { skip: !chrome.findChrome() },
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
        res.end(
          '<!doctype html><button id="opener">Export</button><wb-export-dialog></wb-export-dialog><script>document.querySelector("wb-export-dialog").pages=[{id:"early",label:"Early"}]</script><script type="module">import {WbExportDialog} from "/_workbench/ui/src/views/export/components/export-dialog/element.js"; import {registerExportComponents} from "/_workbench/ui/src/views/export/register-export.js"; window.importRegistered=!!customElements.get("wb-export-dialog"); registerExportComponents(); registerExportComponents(); window.registeredClass=customElements.get("wb-export-dialog")===WbExportDialog;</script>',
        );
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
    const result = await target.evaluate(`(async()=>{
    await Promise.race([customElements.whenDefined('wb-export-dialog'), new Promise((_, reject)=>setTimeout(()=>reject(new Error('Dialog registration failed')),3000))]);
    if(window.importRegistered || !window.registeredClass) throw new Error('Registration must be explicit and repeatable');
    const el=document.querySelector('wb-export-dialog'), events=[];
    if(el.pages[0].id!=='early') throw new Error('Input before registration was lost');
    el.pages=[{id:'a',label:'A'},{id:'b',label:'B'}]; el.collections=['Pages']; el.current={page:'a',artboard:'warning',iteration:'bold',width:777,height:600};
    el.addEventListener('wb-export-request',e=>events.push(e.detail));
    const root=el.shadowRoot, query=root.querySelector.bind(root), field=n=>query('[name='+n+']');
    const native=query('dialog'),form=query('form'),cancel=query('button[type=button]');
    let queries=0;root.querySelector=(...args)=>{queries++;return query(...args)};
    document.getElementById('opener').focus(); el.show();
    const sourceDefault=field('variants').value;
    field('format').value='pdf';field('format').dispatchEvent(new Event('change',{bubbles:true}));
    const visualDefault=field('variants').value;
    form.requestSubmit();
    el.show();field('scope').value='pages';field('scope').dispatchEvent(new Event('change',{bubbles:true}));
    field('pages').options[1].selected=true;form.requestSubmit();
    el.show();cancel.click();
    if(queries)throw Error('Repeated presentation must use retained form references');
    document.getElementById('opener').focus();el.show();
    native.dispatchEvent(new MouseEvent('click',{bubbles:true}));
    if(!native.open)throw Error('Export retains its existing backdrop policy');
    native.dispatchEvent(new Event('cancel',{cancelable:true}));
    if(native.open||document.activeElement.id!=='opener')throw Error('Native cancellation restores the opener');
    let closes=0;const close=native.close.bind(native);native.close=()=>{closes++;close()};
    el.show();el.remove();if(native.open)throw Error('Disconnect must close the modal');
    closes=0;native.dispatchEvent(new Event('cancel',{cancelable:true}));
    if(closes)throw Error('Disconnected listeners must be released');
    document.body.append(el);el.show();closes=0;
    native.dispatchEvent(new Event('cancel',{cancelable:true}));
    if(closes!==1)throw Error('Reconnect must install one cancellation listener');
    return {sourceDefault,visualDefault,events,closed:!native.open};
  })()`);
    assert.equal(result.sourceDefault, 'all');
    assert.equal(result.visualDefault, 'current');
    assert.equal(result.events[0].format, 'pdf');
    assert.equal(result.events[0].current.width, 777);
    assert.equal(result.events[0].current.artboard, 'warning');
    assert.equal(result.events[0].current.iteration, 'bold');
    assert.deepEqual(result.events[1].pages, ['b']);
    assert.equal(result.events.length, 2);
    assert.ok(result.closed);
  },
);
