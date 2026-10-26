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
            "default-src 'none'; script-src 'self' 'nonce-test'; style-src 'self'; img-src 'self' data:; connect-src 'self'",
          );
          res.end(`<!doctype html><script type="importmap" nonce="test">{"imports":{"@canonic2/lattice":"/_workbench/node_modules/@canonic2/lattice/src/index.js","@canonic2/lattice/elements":"/_workbench/node_modules/@canonic2/lattice/src/modules/elements/elements.js"}}</script>${styles}<div class="wb">${header}<input id="retained" value="Acme edit"></div><wb-size-dialog id="sizeDialog"></wb-size-dialog><wb-sizes-editor id="sizesEditor"></wb-sizes-editor>
          <script type="module" src="/fixture.js"></script>`);
        } else if (req.url === '/fixture.js') {
          res.setHeader('Content-Type', 'text/javascript');
          res.end(`import {registerSizeComponents} from '/_workbench/ui/src/views/top-bar/register-sizes.js';
          import {registerActionsComponents} from '/_workbench/ui/src/views/top-bar/register-actions.js';
          registerSizeComponents();registerActionsComponents();
          import {createTopBar,createSizeControls} from '/_workbench/ui/src/modules/top-bar/index.js';
          import {bindTopBar} from '/_workbench/ui/src/views/top-bar/bindings.js';
          import {bindSizes} from '/_workbench/ui/src/views/top-bar/size-bindings.js';
          import {bindResponsiveTopBar} from '/_workbench/ui/src/views/top-bar/top-bar.js';
          window.calls=[];window.routes=[];window.lifetime=new AbortController();
          window.fixtureBindings={bindTopBar,bindSizes,createTopBar,createSizeControls};
          const signal=lifetime.signal,report=error=>{throw error},$=id=>document.getElementById(id);
          const sizesOwner=createSizeControls({choose:key=>calls.push(key),context:()=>({allowPageOnly:true,notice:'Acme notice',page:'account.html',docs:false}),sizes:()=>[],post:body=>{window.posted=body;return new Promise((resolve,reject)=>{window.saveSize=resolve;window.rejectSize=reject})},added:()=>calls.push('added'),updated:()=>calls.push('updated'),report});
          bindSizes({switcher:$('sizeSwitcher'),add:$('sizeDialog'),editor:$('sizesEditor')},sizesOwner,{signal,report});
          signal.addEventListener('abort',sizesOwner.dispose,{once:true});
          window.bar=createTopBar({writeHash:(...args)=>routes.push(args),size:()=> 'laptop',actionsChanged:()=>calls.push(bar.state.get().actionsOn),reload:()=>calls.push('reload binding')});
          bindTopBar({lensesBox:$('lenses'),actions:$('actionsToggle'),reload:$('reload'),open:$('open'),crumb:$('crumb'),pageName:$('crumbPage'),
            scenario:{control:$('scenarioControl'),button:$('scenarioButton'),menu:$('scenarioMenu'),name:$('scenarioName')},
            source:{control:$('sourceControl'),button:$('openSource'),menu:$('sourceMenu')}},bar,{signal,report});
          const topbar=document.querySelector('.wb-topbar');
          window.responsive=bindResponsiveTopBar({topbar,left:topbar.querySelector('.wb-topbar-left'),center:topbar.querySelector('.wb-topbar-center'),right:topbar.querySelector('.wb-topbar-right'),
            overflow:$('topbarOverflow'),button:$('topbarOverflowButton'),menu:$('topbarOverflowMenu'),sourceMenu:$('sourceMenu'),
            controls:Object.fromEntries(['reload','copyReference','open','previewControls','configureSpace','exportSpace'].map(id=>[id,$(id)]))},{signal});
          window.fixtureItem={src:'account.html',label:'Acme account settings',scenarios:[{id:'default',label:'Default'},{id:'empty',label:'Empty'}],implementations:{storybook:{title:'Acme'}}};
          window.fixtureImplementations={storybook:{key:'storybook',label:'Storybook',kind:'storybook'}};
          bar.setPage(fixtureItem.src);bar.showPage(fixtureItem);bar.showLenses(fixtureItem,fixtureImplementations,null);bar.showScenarios(fixtureItem,null,fixtureItem.scenarios);
          for(const id of ['reload','copyReference','previewControls','configureSpace','exportSpace']){
            const button=document.getElementById(id);button.hidden=false;button.addEventListener('click',()=>calls.push(id));
          }
          document.getElementById('crumb').hidden=false;
          document.getElementById('crumbPage').textContent='Acme account settings';
          const sizes=document.getElementById('sizeSwitcher');
          sizes.sizes=[{key:'laptop',label:'Laptop',kind:'fixed',button:true,width:1512,height:982},{key:'mobile',label:'Mobile',kind:'fixed',button:true,width:393,height:852}];sizes.current='laptop';

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
    await until('window.fixtureReady && !!window.responsive');
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
    const retained = await target.evaluate(`(()=>{
      const lenses=document.getElementById('lenses'),scenarios=document.getElementById('scenarioMenu');
      const lens=lenses.firstChild,scenario=scenarios.firstChild,watch=new MutationObserver(()=>{});
      watch.observe(lenses,{subtree:true,childList:true,attributes:true,characterData:true});
      watch.observe(scenarios,{subtree:true,childList:true,attributes:true,characterData:true});
      bar.setActions(true);bar.setActions(true);
      bar.showLenses(fixtureItem,{...fixtureImplementations},null);
      bar.showScenarios(fixtureItem,null,fixtureItem.scenarios.map(row=>({...row})));
      const writes=watch.takeRecords().length;watch.disconnect();
      return {lens:lens===lenses.firstChild,scenario:scenario===scenarios.firstChild,writes};
    })()`);
    assert.deepEqual(retained, { lens: true, scenario: true, writes: 0 });
    const keyboard = await target.evaluate(`(()=>{
      const button=document.getElementById('scenarioButton'),menu=document.getElementById('scenarioMenu');
      button.focus();button.dispatchEvent(new KeyboardEvent('keydown',{key:'ArrowDown',bubbles:true,cancelable:true}));
      const first=document.activeElement===menu.firstChild;
      menu.firstChild.dispatchEvent(new KeyboardEvent('keydown',{key:'End',bubbles:true,cancelable:true}));
      const last=document.activeElement===menu.lastChild;menu.lastChild.click();
      return {first,last,closed:menu.hidden,focused:document.activeElement===button,route:routes.at(-1)};
    })()`);
    assert.deepEqual(keyboard, {
      first: true,
      last: true,
      closed: true,
      focused: true,
      route: ['account.html', 'empty', 'laptop', null, null, null],
    });
    const rollback = await target.evaluate(`(()=>{
      const {bindTopBar,bindSizes,createTopBar,createSizeControls}=fixtureBindings;
      const node=()=>document.createElement('div'),button=()=>document.createElement('button');
      const menu=()=>({control:node(),button:button(),menu:node(),name:node()});
      const elements={lensesBox:node(),actions:node(),reload:button(),open:document.createElement('a'),crumb:node(),pageName:node(),scenario:menu(),source:menu()};
      const failure=Error('initial assignment failed');
      Object.defineProperty(elements.actions,'disabled',{set(){throw failure}});
      const owner=createTopBar();let topBarFailed=false,sizesFailed=false;
      try{bindTopBar(elements,owner,{signal:new AbortController().signal,report:error=>{throw error}})}catch(error){topBarFailed=error===failure}
      owner.showLenses(fixtureItem,fixtureImplementations,null);
      const sizes=createSizeControls({context:()=>({}),sizes:()=>[]});
      const add=node(),editor=node();Object.defineProperty(editor,'sizes',{set(){throw failure}});
      try{bindSizes({switcher:button(),add,editor},sizes,{signal:new AbortController().signal,report:error=>{throw error}})}catch(error){sizesFailed=error===failure}
      sizes.openAdd();
      return {topBarFailed,sizesFailed,unwatched:elements.lensesBox.children.length===0,closed:!add.open};
    })()`);
    assert.deepEqual(rollback, {
      topBarFailed: true,
      sizesFailed: true,
      unwatched: true,
      closed: true,
    });
    await target.evaluate(`document.getElementById('scenarioButton').click();lifetime.abort()`);
    assert.equal(await target.evaluate(`document.getElementById('scenarioMenu').hidden`), true);
    const stopped = await target.evaluate(`(()=>{
      const before=calls.filter(call=>call==='reload binding').length;
      bar.showLenses(null,{},null);bar.setOpenLink('https://example.com/late');
      document.getElementById('reload').click();
      return {retained:document.getElementById('lenses').children.length===2,href:document.getElementById('open').hasAttribute('href'),quiet:calls.filter(call=>call==='reload binding').length===before};
    })()`);
    assert.deepEqual(stopped, { retained: true, href: false, quiet: true });
    await resize(600);
    await target.evaluate(
      `new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))`,
    );
    assert.equal(
      await target.evaluate(
        `document.querySelector('.wb-topbar').classList.contains('is-topbar-compact')`,
      ),
      false,
    );
    assert.deepEqual(errors, []);
  },
);
