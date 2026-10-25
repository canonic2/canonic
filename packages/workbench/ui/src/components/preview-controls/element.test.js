import test from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { createUiAssets } from '../../../../scripts/ui-browser-fixture.cjs';
const require = createRequire(import.meta.url);
const { Browser, findChrome } = require('../../../../scripts/chrome.cjs');

test(
  'preview controls preserve native inputs, focus and binding lifetime under CSP',
  { skip: !findChrome() },
  async (t) => {
    const assets = createUiAssets(
      path.resolve(fileURLToPath(new URL('../../../../', import.meta.url))),
    );
    const model = {
      event: 'ready',
      id: 'acme',
      scenario: 'default',
      docs: 'Acme documentation',
      controls: {
        text: { type: 'text' },
        number: { type: 'number', min: 0, max: 10, step: 2 },
        boolean: { type: 'boolean' },
        select: { type: 'select', options: [1, 2] },
        json: { type: 'json' },
      },
      inputs: { text: 'Continue', number: 2, boolean: true, select: 1, json: { title: 'Acme' } },
      actions: [],
    };
    const server = createServer((req, res) => {
      void (async () => {
        if (req.url.startsWith('/_workbench/')) {
          const asset = await assets(req.url);
          res.writeHead(asset.status, { 'Content-Type': asset.type });
          res.end(asset.body);
          return;
        }
        res.setHeader('Content-Type', 'text/html');
        res.setHeader(
          'Content-Security-Policy',
          "default-src 'none'; script-src 'self' 'nonce-test'; style-src 'self'; connect-src 'self'",
        );
        res.end(`<!doctype html><script type="importmap" nonce="test">{"imports":{"@canonic2/lattice":"/_workbench/node_modules/@canonic2/lattice/src/index.js","@canonic2/lattice/elements":"/_workbench/node_modules/@canonic2/lattice/src/modules/elements/elements.js"}}</script>
        <wb-preview-controls id="early"></wb-preview-controls><button id="button">Preview controls</button><wb-preview-controls id="panel"></wb-preview-controls>
        <script nonce="test">window.violations=[];addEventListener('securitypolicyviolation',e=>violations.push(e.violatedDirective));
        window.sample=${JSON.stringify(model)};document.getElementById('early').model=sample;</script>
        <script type="module" nonce="test">
        import {registerPreviewControls} from '/_workbench/ui/src/components/preview-controls/register.js';
        import {createPreviewControls} from '/_workbench/ui/src/modules/preview-controls/index.js';
        import {bindPreviewControls} from '/_workbench/ui/src/views/top-bar/preview-bindings.js';
        registerPreviewControls();window.sent=[];window.errors=[];window.lifetime=new AbortController();
        window.controls=createPreviewControls({send:(...args)=>sent.push(args)});
        controls.receive('one',sample);controls.choose('one');
        bindPreviewControls({controls,button:document.getElementById('button'),panel:document.getElementById('panel'),signal:lifetime.signal,report:e=>errors.push(e.message)});
        window.ready=true;</script>`);
      })().catch((error) => {
        res.statusCode = 500;
        res.end(String(error));
      });
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    const browser = new Browser();
    t.after(async () => {
      await browser.close();
      await new Promise((resolve) => server.close(resolve));
    });
    await browser.launch();
    const target = await browser.createTarget();
    await target.navigate(`http://127.0.0.1:${server.address().port}/`);
    const deadline = Date.now() + 5000;
    while (!(await target.evaluate('window.ready===true'))) {
      if (Date.now() > deadline) throw Error('Preview controls did not register');
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    await target.evaluate(`(()=>{
    const check=(ok,text)=>{if(!ok)throw Error(text)};
    const early=document.getElementById('early'),panel=document.getElementById('panel'),button=document.getElementById('button');
    const field=name=>panel.shadowRoot.querySelector('[data-name="'+name+'"]');
    check(early.shadowRoot.querySelectorAll('label').length===5,'pre-upgrade model replays');
    check(sent.length===0,'inputs are silent');check(panel.hidden,'starts closed');button.click();
    check(!panel.hidden&&button.getAttribute('aria-expanded')==='true','open binding');
    check(field('number').min==='0'&&field('number').max==='10'&&field('number').step==='2','number constraints');
    for(const [name,value] of [['text','Edited'],['number','4'],['boolean',false],['select','2'],['json','{"ok":true}']]){
      const input=field(name);if(name==='boolean')input.checked=value;else input.value=value;
      input.dispatchEvent(new Event('change',{bubbles:true}));
    }
    check(sent.length===5,'one command per change');
    const inputs=sent.at(-1)[1].inputs;
    check(inputs.text==='Edited'&&inputs.number===4&&!inputs.boolean&&inputs.select===2&&inputs.json.ok,'native values');
    field('json').value='{';field('json').dispatchEvent(new Event('change',{bubbles:true}));
    check(sent.length===5&&!field('json').checkValidity(),'invalid JSON sends no intent');
    const text=field('text');text.focus();text.value='Draft';text.setSelectionRange(2,2);
    controls.receive('one',{event:'action',id:'acme',name:'clicked',values:[1]});
    check(field('text')===text&&panel.shadowRoot.activeElement===text&&text.value==='Draft'&&text.selectionStart===2,'actions preserve draft and focus');
    check(panel.shadowRoot.querySelector('li').textContent==='clicked: 1','action label');
    panel.shadowRoot.querySelector('details').open=true;
    controls.receive('one',{event:'action',id:'acme',name:'again'});
    check(panel.shadowRoot.querySelector('details').open,'documentation disclosure survives');
    panel.shadowRoot.querySelector('button').click();
    check(sent.at(-1)[1].command==='reset'&&Object.keys(sent.at(-1)[1].inputs).length===0,'reset asks the capability');
    check(field('json').checkValidity(),'reset clears local invalid text');
    panel.remove();document.body.append(panel);const before=sent.length;
    field('text').value='Reconnected';field('text').dispatchEvent(new Event('change',{bubbles:true}));
    check(sent.length===before+1,'reconnect has one listener');
    check(early.model.inputs.text==='Continue','another instance stays independent');
    controls.receive('one',{...sample,controls:Object.fromEntries(Object.entries(sample.controls).reverse())});
    check(panel.shadowRoot.querySelector('label span').textContent==='json','updated definitions keep their authored control order');
    lifetime.abort();const hidden=panel.hidden;controls.toggle();check(panel.hidden===hidden,'aborted binding stops');
    check(!errors.length&&!violations.length,'no errors or CSP violations');
  })()`);
    assert.deepEqual(await target.evaluate('errors'), []);
  },
);
