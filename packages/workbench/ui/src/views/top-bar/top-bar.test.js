import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { createUiAssets } from '../../../../scripts/ui-browser-fixture.cjs';

const { Browser, findChrome } = createRequire(import.meta.url)('../../../../scripts/chrome.cjs');

test(
  'the authored top bar keeps its responsive menu and forwards existing controls',
  {
    skip: !findChrome() && 'Chrome is required',
  },
  async (t) => {
    const browser = new Browser();
    const assets = createUiAssets(
      path.resolve(fileURLToPath(new URL('../../../../', import.meta.url))),
    );
    const document = await readFile(
      new URL('../../app/workbench/index.html', import.meta.url),
      'utf8',
    );
    const start = document.indexOf('<header class="wb-topbar">');
    const header = document.slice(start, document.indexOf('</header>', start) + '</header>'.length);
    const styles = [
      ...document.matchAll(/<link rel="stylesheet" href="(\/_workbench\/ui\/src\/[^"]+)"\s*\/>/g),
    ]
      .map((match) => match[0])
      .join('');
    const server = createServer((req, res) => {
      void (async () => {
        if (req.url === '/') {
          res.setHeader('Content-Type', 'text/html');
          res.setHeader(
            'Content-Security-Policy',
            "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; connect-src 'self'",
          );
          res.end(`<!doctype html>${styles}<div class="wb">${header}<input id="retained" value="Acme edit"></div><wb-size-dialog id="sizeDialog"></wb-size-dialog><wb-sizes-editor id="sizesEditor"></wb-sizes-editor>
          <script type="module" src="/fixture.js"></script>
          <script src="/_workbench/ui/src/views/top-bar/size-bindings.js"></script>
          <script src="/_workbench/ui/src/views/top-bar/bindings.js"></script>
          <script src="/_workbench/ui/src/views/top-bar/top-bar.js"></script>`);
        } else if (req.url === '/fixture.js') {
          res.setHeader('Content-Type', 'text/javascript');
          res.end(`import {registerSizeComponents} from '/_workbench/ui/src/views/top-bar/register-sizes.js';
          import {registerActionsComponents} from '/_workbench/ui/src/views/top-bar/register-actions.js';
          registerSizeComponents();registerActionsComponents();
          window.calls=[];
          wbSizeBindings({choose:key=>calls.push(key),context:()=>({allowPageOnly:true,notice:'Acme notice',page:'account.html',docs:false}),sizes:()=>[],post:body=>{window.posted=body;return new Promise((resolve,reject)=>{window.saveSize=resolve;window.rejectSize=reject})},added:()=>calls.push('added'),updated:()=>calls.push('updated')});
          wbTopBarBindings({actions:checked=>calls.push(checked),reload:()=>calls.push('reload binding'),openSources:open=>{document.getElementById('sourceMenu').hidden=!open},openScenarios:open=>{document.getElementById('scenarioMenu').hidden=!open}});
          for(const id of ['reload','copyReference','previewControls','configureSpace','exportSpace']){
            const button=document.getElementById(id);button.hidden=false;button.addEventListener('click',()=>calls.push(id));
          }
          document.getElementById('crumb').hidden=false;
          document.getElementById('crumbPage').textContent='Acme account settings';
          const sizes=document.getElementById('sizeSwitcher');
          sizes.sizes=[{key:'laptop',label:'Laptop',kind:'fixed',button:true,width:1512,height:982},{key:'mobile',label:'Mobile',kind:'fixed',button:true,width:393,height:852}];sizes.current='laptop';
          const lenses=document.getElementById('lenses');lenses.hidden=false;
          lenses.innerHTML='<button class="wb-lens" aria-pressed="true">Design</button><button class="wb-lens">Storybook</button>';
          window.fixtureReady=true;`);
        } else {
          const asset = await assets(req.url);
          res.writeHead(asset.status, { 'Content-Type': asset.type });
          res.end(asset.body);
        }
      })().catch((error) => {
        res.statusCode = 500;
        res.end(String(error));
      });
    });
    t.after(async () => {
      await browser.close();
      await new Promise((resolve) => server.close(resolve));
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    await browser.launch();
    const target = await browser.createTarget();
    const errors = [];
    target.on('Runtime.exceptionThrown', (event) =>
      errors.push(event.exceptionDetails.exception?.description || event.exceptionDetails.text),
    );
    const resize = (width) =>
      target.send('Emulation.setDeviceMetricsOverride', {
        width,
        height: 900,
        deviceScaleFactor: 1,
        mobile: false,
      });
    async function until(expression) {
      const deadline = Date.now() + 5000;
      while (!(await target.evaluate(expression))) {
        if (Date.now() > deadline)
          throw Error('Top bar did not settle: ' + expression + '\n' + errors.join('\n'));
        await new Promise((resolve) => setTimeout(resolve, 20));
      }
    }
    await resize(1800);
    await target.navigate('http://127.0.0.1:' + server.address().port);
    await until('window.fixtureReady && !!window.wbTopBar');
    await until(`!document.querySelector('.wb-topbar').classList.contains('is-topbar-compact')`);
    assert.equal(
      await target.evaluate(`getComputedStyle(document.querySelector('.wb-topbar')).display`),
      'grid',
    );
    await resize(600);
    await until(`document.querySelector('.wb-topbar').classList.contains('is-topbar-compact')`);
    const result = await target.evaluate(`(()=>{
    const button=document.getElementById('topbarOverflowButton'),menu=document.getElementById('topbarOverflowMenu');
    const hidden=getComputedStyle(document.getElementById('reload')).display==='none';
    const sizeVisible=document.getElementById('sizeSwitcher').getBoundingClientRect().width>0;
    button.click();
    const labels=[...menu.querySelectorAll('button')].map(row=>row.textContent.trim());
    [...menu.querySelectorAll('button')].find(row=>row.textContent.trim()==='Configure pages').click();
    const closed=menu.hidden&&button.getAttribute('aria-expanded')==='false';
    button.click();document.dispatchEvent(new KeyboardEvent('keydown',{key:'Escape'}));
    const escaped=menu.hidden;
    button.click();document.body.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));
    return {hidden,sizeVisible,labels,calls,closed,escaped,outside:menu.hidden};
  })()`);
    assert.equal(result.hidden, true);
    assert.equal(result.sizeVisible, true);
    assert.ok(result.labels.includes('Reload'));
    assert.ok(result.labels.includes('Configure pages'));
    assert.deepEqual(result.calls, ['configureSpace']);
    assert.equal(result.closed && result.escaped && result.outside, true);
    await resize(1800);
    await until(`!document.querySelector('.wb-topbar').classList.contains('is-topbar-compact')`);
    assert.notEqual(
      await target.evaluate(`getComputedStyle(document.getElementById('reload')).display`),
      'none',
    );
    assert.equal(await target.evaluate(`document.getElementById('retained').value`), 'Acme edit');
    await target.evaluate(
      `document.getElementById('sizeSwitcher').allowEdit=true;document.getElementById('sizeSwitcher').shadowRoot.querySelector('[data-action=custom]').click()`,
    );
    await until(`document.getElementById('sizeDialog').open`);
    assert.equal(
      await target.evaluate(`document.getElementById('sizeDialog').notice`),
      'Acme notice',
    );
    await target.evaluate(
      `document.getElementById('sizeDialog').dispatchEvent(new CustomEvent('wb-size-add',{detail:{name:'Acme size',width:640,height:480}}));rejectSize(new Error('Acme write refused'))`,
    );
    await until(
      `document.getElementById('sizeDialog').error==='Acme write refused' && !document.getElementById('sizeDialog').pending`,
    );
    assert.equal(await target.evaluate(`document.getElementById('sizeDialog').open`), true);
    assert.equal(await target.evaluate(`window.posted.size.page`), 'account.html');
    await target.evaluate(
      `document.getElementById('sizeDialog').dispatchEvent(new CustomEvent('wb-size-add',{detail:{name:'Acme size',width:640,height:480}}));saveSize({key:'acme'})`,
    );
    await until(`!document.getElementById('sizeDialog').open && calls.includes('added')`);
    assert.deepEqual(errors, []);
  },
);
