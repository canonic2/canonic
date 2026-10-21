/* Real attached-frame retention check. Chrome is only the test driver. */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const http = require('node:http');
const { performance } = require('node:perf_hooks');
const server = require('../server/node/server');
const { Browser } = require('./chrome.cjs');

async function main() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-preview-sessions-smoke-'));
  const browser = new Browser();
  let running;
  let catalogDelay = 0;
  let includeMissing = false;
  const requests = [];
  const app = http.createServer((req, res) => {
    requests.push(req.url);
    res.setHeader('Content-Type', 'text/html');
    if (req.url === '/index.json') {
      res.setHeader('Content-Type', 'application/json');
      const body = JSON.stringify({ entries: Object.fromEntries((includeMissing ? ['Page', 'Other', 'Missing'] : ['Page', 'Other']).flatMap(title => ['default', 'secondary'].map(state => [
        title.toLowerCase() + '--' + state, { id: title.toLowerCase() + '--' + state, type: 'story', title: 'Components/' + title, name: state === 'default' ? 'Text field' : 'Error field' },
      ]))) });
      if (catalogDelay) setTimeout(() => res.end(body), catalogDelay);
      else res.end(body);
      return;
    }
    res.end(`<!doctype html><html><body><h1>Acme lens</h1><input id="edit" value="Original">
      <details id="menu"><summary>Menu</summary>Open menu</details><div style="height:2500px"></div>
      <script>
      function announce(type,args){parent.postMessage(JSON.stringify({key:'storybook-channel',event:{type,args}}),'*')}
      function render(id){announce('currentStoryWasSet',[{storyId:id}]);setTimeout(()=>{window.story=id;announce('storyRendered',[id])},250)}
      addEventListener('message',event=>{let data=event.data;try{if(typeof data==='string')data=JSON.parse(data)}catch{return}
        if(data?.type==='fixture-command'){
          if(data.key)announce('previewKeydown',[{event:{key:data.key,code:'Equal',metaKey:true,ctrlKey:false,altKey:false,shiftKey:false}}]);
          if(data.value!==undefined)document.querySelector('#edit').value=data.value;
          if(data.menu!==undefined)document.querySelector('#menu').open=data.menu;
          if(data.navigate)parent.postMessage({type:'wb-preview-navigate',href:new URL(typeof data.navigate==='string'?data.navigate:'/missing',location.origin).href},'*');
          parent.postMessage({type:'fixture-state',token:data.token,value:document.querySelector('#edit').value,
            menu:document.querySelector('#menu').open,story:window.story,viewport:{width:innerWidth,height:innerHeight}},'*');return;}
        if(data?.key==='storybook-channel'&&data.event?.type==='setCurrentStory')render(data.event.args[0].storyId)});
      if(location.pathname==='/iframe.html')addEventListener('load',()=>render(new URL(location.href).searchParams.get('id')));
      </script></body></html>`);
  });
  await new Promise(resolve => app.listen(0, '127.0.0.1', resolve));
  const origin = 'http://127.0.0.1:' + app.address().port;
  fs.symlinkSync(path.resolve(__dirname, "../node_modules"), path.join(root, 'node_modules'), 'junction');
  fs.writeFileSync(path.join(root, 'page.workbench.ts'), `import {definePreview} from '@canonic2/workbench';
    export default definePreview({id:'pages/page',adapter:'react',source:{entry:'./page.tsx'},
      states:{default:{}},controls:{label:{type:'text'}},inputs:{label:'Original'}});`);
  fs.writeFileSync(path.join(root, 'page.tsx'), `import React from 'react';
    export default function Page(props){const [count,setCount]=React.useState(0);
      React.useLayoutEffect(()=>{window.mounts=(window.mounts||0)+1},[]);
      return <main><h1>Acme React</h1><input id="edit" defaultValue={props.label}/>
        <details id="menu"><summary>Menu</summary>Open menu</details>
        <button id="counter" onClick={()=>setCount(count+1)}>{count}</button>
        <div style={{height:2500}}/></main>}`);
  fs.writeFileSync(path.join(root, 'workbench.yaml'), `name: Acme
implementations:
  storybook:
    kind: storybook
    url: ${origin}
  app:
    kind: url
    base: ${origin}
collections:
  - name: Pages
    items:
      - label: Page
        src: page.workbench.ts
        docs: page.md
        implementations:
          storybook: Components/Page
          app: /page
      - label: Other
        src: other.md
        implementations:
          storybook: Components/Other
      - label: Feedback
        src: feedback.workbench.ts
      - label: Broken
        src: broken.html
        implementations:
          storybook: Components/Missing
`);
  fs.writeFileSync(path.join(root, 'other.md'), '# Other documentation');
  fs.writeFileSync(path.join(root, 'broken.html'), '<!doctype html><body>Broken authored page');
  fs.writeFileSync(path.join(root, 'feedback.workbench.ts'), `import {definePreview} from '@canonic2/workbench';export default definePreview({id:'feedback',adapter:'html',source:{entry:'./feedback.ts'},artboards:${JSON.stringify(Object.fromEntries(['info','success','warning','danger','autopilot'].map(id => [id,{label:id,width:320,height:120}])))} });`);
  fs.writeFileSync(path.join(root, 'feedback.ts'), `export default function(canvas,context){canvas.textContent=context.artboard;}`);
  fs.writeFileSync(path.join(root, 'page.md'), '# Acme documentation\n\n' + 'Page documentation.\n\n'.repeat(150));
  try {
    running = await server.start({ root, capture: { warm() {}, prepare() {}, preparePage() {}, close() {} } });
    console.log('workbench on ' + running.url);
    await browser.launch();
    const target = await browser.createTarget();
    const failures = [];
    target.on('Runtime.exceptionThrown', event => failures.push(event.exceptionDetails.text));
    await target.send('Emulation.setDeviceMetricsOverride', { width: 1900, height: 1100, deviceScaleFactor: 1, mobile: false });
    await target.send('Page.addScriptToEvaluateOnNewDocument', { source:
      `window.changes=0;window.fixtureReports=[];window.labelTimeline=[];window.loadingFrames=[];
      addEventListener('wb-frame-change',()=>changes++);
      addEventListener('message',event=>{if(event.data?.type==='fixture-state')fixtureReports.push(event.data)});
      let previousLabels='';
      function sampleLabels(){
        const loading=document.getElementById('blank');
        if(loading && !loading.hidden && loading.classList.contains('is-loading')) loadingFrames.push(location.hash);
        const canvas=document.getElementById('artboards');
        if(canvas && getComputedStyle(canvas).display!=='none' && getComputedStyle(canvas).visibility==='visible'){
          const sample={page:document.getElementById('crumbPage').textContent,title:document.title,
            story:document.getElementById('stateControl').hidden?null:document.getElementById('stateName').textContent,
            preset:document.getElementById('sizeSwitcher').current,size:canvas.state.artboards[0]?.size,
            labels:[...canvas.shadowRoot.querySelectorAll('section button')].map(b=>b.textContent)};
          const serial=JSON.stringify(sample);if(serial!==previousLabels){labelTimeline.push(sample);previousLabels=serial;}
        }
        requestAnimationFrame(sampleLabels);
      }
      requestAnimationFrame(sampleLabels);` });
    async function until(expression) {
      const deadline = Date.now() + 30000;
      while (Date.now() < deadline) {
        if (await target.evaluate(expression)) return;
        await new Promise(resolve => setTimeout(resolve, 20));
      }
      throw new Error('Retained preview did not settle: ' + expression + '\n' + await target.evaluate(`JSON.stringify({hash:location.hash,state:document.getElementById('artboards')?.state,reports:window.reports,fixtureReports:document.querySelector('wb-canvas > iframe')?.contentWindow.fixtureReports})`));
    }
    async function pick(lens, state = 'default', expectPending = false) {
      const started = performance.now();
      await target.evaluate('location.hash=' + JSON.stringify('page.workbench.ts:' + state + '@laptop~' + (lens || 'design')));
      if (expectPending) {
        await until(`${canvas}.artboards[0]?.target.state==='secondary' && !${canvas}.artboards[0]?.view?.ready`);
        assert.equal(await target.evaluate('getComputedStyle(document.getElementById("artboards")).visibility'), 'hidden', 'outgoing pixels stay private while the requested story renders');
        await until(`!document.getElementById('blank').hidden && document.getElementById('blank').classList.contains('is-loading')`);
        assert.deepEqual(await loadingStatus(), {hidden:false,text:'Loading preview…',loading:true,busy:'true'}, 'slow story rendering shows the loading status');
        assert.equal(await target.evaluate('document.title'), 'Page · Error field · Storybook · Acme');
        await until(`${canvas}.artboards[0].view.label==='Page / Error field' && !${canvas}.artboards[0].view.ready`);
      }
      await until(`${canvas}.artboards[0]?.view?.ready && ${canvas}.artboards[0].target.lens===${JSON.stringify(lens || null)}${lens === 'storybook' ? ` && ${canvas}.artboards[0].target.state===${JSON.stringify(state)}` : ''}`);
      if (lens !== 'docs') {
        assert.deepEqual(await target.evaluate(`${canvas}.artboards[0].size`), {width:1512,height:982}, 'navigation applies its selected Laptop preset to the retained artboard');
        await until(`${renderer}.innerWidth===1512 && ${renderer}.innerHeight===982`);
      }
      assert.equal(await target.evaluate('getComputedStyle(document.getElementById("artboards")).visibility'), 'visible', 'the settled destination is revealed');
      assert.equal((await loadingStatus()).hidden, true, 'ready content clears its loading status');
      assert.equal((await loadingStatus()).busy, 'false');
      assert.deepEqual(await target.evaluate(`({width:${renderer}.document.getElementById('artboardContent').clientWidth,height:${renderer}.document.getElementById('artboardContent').clientHeight,nestedCanvas:!!${renderer}.document.getElementById('canvas')})`), {...await target.evaluate(`({width:${renderer}.innerWidth,height:${renderer}.innerHeight})`),nestedCanvas:false}, 'every lens uses the complete owning viewport without an inner canvas');
      if (lens === 'storybook') {
        const name = state === 'default' ? 'Text field' : 'Error field';
        assert.deepEqual(await target.evaluate(`({title:document.title,page:document.getElementById('crumbPage').textContent,story:document.getElementById('stateName').textContent,label:${canvas}.artboards[0].view.label,target:${canvas}.artboards[0].target.label,frame:document.querySelector('wb-canvas > iframe').title})`), {title:`Page · ${name} · Storybook · Acme`,page:'Page',story:name,label:`Page / ${name}`,target:`Page / ${name}`,frame:`Page / ${name}`});
      }
      if (lens === 'docs') await readingSurface();
      console.log(JSON.stringify({ lens: lens || 'workbench', state, ms: Math.round(performance.now() - started), storybookDocuments: requests.filter(url=>url.startsWith('/iframe.html')).length }));
    }
    const canvas = 'document.getElementById("artboards").state';
    const renderer = 'document.querySelector("wb-canvas > iframe").contentWindow';
    const active = renderer + '.document.querySelector("iframe.is-active")';
    const page = active + '.contentWindow';
    const loadingStatus = () => target.evaluate('({hidden:document.getElementById("blank").hidden,text:document.getElementById("blank").textContent,loading:document.getElementById("blank").classList.contains("is-loading"),busy:document.getElementById("canvas").getAttribute("aria-busy")})');
    async function readingSurface() {
      assert.deepEqual(await target.evaluate(`(()=>{
        const canvas=document.getElementById('artboards'), shell=canvas.shadowRoot.querySelector('section');
        const bounds=canvas.getBoundingClientRect(), page=shell.getBoundingClientRect();
        return {label:shell.querySelector('button').getClientRects().length>0,
          shadow:getComputedStyle(shell).boxShadow,scale:canvas.scale,
          fills:Math.abs(page.x-bounds.x)<1 && Math.abs(page.y-bounds.y)<1 && Math.abs(page.width-bounds.width)<1 && Math.abs(page.height-bounds.height)<1};
      })()`), {label:false,shadow:'none',scale:1,fills:true}, 'standalone Docs is an undecorated reading surface filling the canvas at 100%');
    }
    let token = 0;
    async function external(command = {}, which = active) {
      command = { ...command, token: ++token, type: 'fixture-command' };
      await target.evaluate(which + '.contentWindow.postMessage(' + JSON.stringify(command) + ',"*")');
      await until(renderer + '.fixtureReports.some(report=>report.token===' + token + ')');
      return target.evaluate(renderer + '.fixtureReports.find(report=>report.token===' + token + ')');
    }
    // Start at docs' canvas-sized viewport, then reuse that renderer for a
    // Storybook page whose ordinary preset is smaller than the docs surface.
    await target.navigate(running.url + 'index.html#page.workbench.ts~docs');
    await until(`${canvas}.artboards[0]?.view?.ready`);
    await readingSurface();
    await target.evaluate(`window.docsFrame=${active};window.docsDocument=docsFrame.contentDocument;docsFrame.contentWindow.scrollTo(0,1000)`);
    await until('docsFrame.contentWindow.scrollY===1000');
    const docsViewport = await target.evaluate(`({width:${renderer}.innerWidth,height:${renderer}.innerHeight})`);
    await target.evaluate('document.getElementById("artboards").zoomTo(.5)');
    await until(`${canvas}.artboards[0].size.height===${docsViewport.height * 2} && ${renderer}.innerHeight===${docsViewport.height * 2}`);
    await until(`docsFrame.contentWindow.scrollY===${Math.max(0, 1000 - docsViewport.height / 2)}`);
    assert.equal(await target.evaluate('docsFrame.contentDocument===docsDocument'), true, 'Docs zoom retains its document and scroll anchor');
    assert.equal(await target.evaluate(`${renderer}.innerWidth`), docsViewport.width, 'Docs zoom preserves layout width');
    const docsScroll = await target.evaluate('docsFrame.contentWindow.scrollY');
    await target.evaluate('document.getElementById("artboards").recenter()');
    assert.equal(await target.evaluate('docsFrame.contentWindow.scrollY'), docsScroll, 'Docs recenter preserves reading position');
    await target.evaluate('document.getElementById("artboards").fitAll()');
    await until(`${renderer}.innerHeight===${docsViewport.height}`);
    await readingSurface();
    await pick('storybook');
    assert.match(await target.evaluate("getComputedStyle(document.getElementById('canvas')).backgroundImage"), /radial-gradient/, 'the outer canvas keeps its grid');
    assert.equal(await target.evaluate(`!!${renderer}.document.querySelector('.wb,.wb-canvas,#canvas,#pageList,#toolbar')`), false, 'the artboard renderer contains no application shell or second canvas');
    assert.deepEqual(await target.evaluate(`({width:${renderer}.document.getElementById('artboardContent').clientWidth,height:${renderer}.document.getElementById('artboardContent').clientHeight})`), await target.evaluate(`({width:${renderer}.innerWidth,height:${renderer}.innerHeight})`), 'the preview fills the complete outer artboard after leaving docs');
    // Retained preview sessions belong to each isolated artboard renderer.
    await pick('');
    await until(`${canvas}.artboards[0]?.view?.ready`);
    const rendererId = await target.evaluate(`${canvas}.artboards[0].id`);
    await target.evaluate(`window.reactFrame=${active};${page}.document.querySelector('#edit').value='Edited React';
      ${page}.document.querySelector('#menu').open=true;${page}.document.querySelector('#counter').click();${page}.scrollTo(0,300);`);
    await until(page + '.document.querySelector("#counter").textContent === "1"');
    await pick('storybook');
    await target.evaluate(`window.storyFrame=${active}`);
    await external({ value: 'Edited Story', menu: true });
    await pick('app');
    await target.evaluate(`window.urlFrame=${active}`);
    await external({ value: 'Edited URL' });
    await pick('');
    assert.deepEqual(await target.evaluate(`({same:${active}===reactFrame,value:${page}.document.querySelector('#edit').value,
      menu:${page}.document.querySelector('#menu').open,count:${page}.document.querySelector('#counter').textContent,
      mounts:${page}.mounts,scroll:${page}.scrollY})`), { same: true, value: 'Edited React', menu: true, count: '1', mounts: 1, scroll: 300 });
    await pick('storybook');
    assert.equal(await target.evaluate(active + '===storyFrame'), true);
    const story = await external();
    assert.equal(story.value, 'Edited Story'); assert.equal(story.menu, true); assert.equal(story.story, 'page--default');
    await pick('storybook', 'secondary', true);
    assert.equal(await target.evaluate(active + '===storyFrame'), true);
    assert.equal((await external()).story, 'page--secondary');
    await target.evaluate('loadingFrames=[]');
    await pick('docs');
    assert.deepEqual(await target.evaluate('loadingFrames'), [], 'a retained docs return never paints the loading message');
    assert.equal(await target.evaluate(`${active}.contentDocument===docsDocument`), true, 'the fast docs return reuses its document');
    await pick('storybook', 'secondary');
    assert.equal(await target.evaluate(active + '===storyFrame'), true, 'docs navigation retains the Storybook session');
    await pick('docs');
    catalogDelay = 1000;
    await target.evaluate('location.hash="#other.md:default@laptop~storybook"');
    await until('document.getElementById("crumbPage").textContent === "Other"');
    assert.equal(await target.evaluate('document.title'), 'Other · Storybook · Acme', 'a pending lookup names its destination rather than the previous docs page');
    assert.equal(await target.evaluate('document.getElementById("stateName").textContent'), '', 'an unresolved story cannot retain the previous state label');
    assert.deepEqual(await target.evaluate('({visibility:getComputedStyle(document.getElementById("artboards")).visibility,inert:document.getElementById("artboards").inert})'), {visibility:'hidden',inert:true}, 'the docs view is concealed as soon as the destination changes, including during catalog lookup');
    await until(`!document.getElementById('blank').hidden && document.getElementById('blank').classList.contains('is-loading')`);
    assert.deepEqual(await loadingStatus(), {hidden:false,text:'Loading preview…',loading:true,busy:'true'}, 'one canvas status covers slow catalog lookup');
    await new Promise(resolve => setTimeout(resolve, 450));
    assert.deepEqual(await loadingStatus(), {hidden:false,text:'Loading preview…',loading:true,busy:'true'});
    assert.equal(await target.evaluate('document.getElementById("toast").textContent.includes("Asking Storybook")'), false, 'slow lookups do not start a competing toast');
    await target.evaluate('document.getElementById("shoot").click()');
    await until('document.getElementById("toast").textContent === "Wait for the requested page to finish loading."');
    await until(`${canvas}.artboards[0]?.target.src==='other.md' && ${canvas}.artboards[0]?.view?.ready`);
    catalogDelay = 0;
    assert.equal(await target.evaluate('getComputedStyle(document.getElementById("artboards")).visibility'), 'visible');
    assert.equal(await target.evaluate(active + '===storyFrame'), true, 'a different mapped page uses the same Storybook document');
    assert.equal((await external()).story, 'other--default');
    const sizeSwitcher = 'document.getElementById("sizeSwitcher")';
    const mobile = `${sizeSwitcher}.shadowRoot.querySelector('button[data-key="mobile"]')`;
    assert.equal(await target.evaluate(`${mobile}.disabled`), false, 'a Markdown-backed page supports viewport sizes in its Storybook lens');
    await target.evaluate(`${mobile}.click()`);
    await until(`${canvas}.artboards[0].size.width===393 && ${canvas}.artboards[0].size.height===852 && ${renderer}.innerWidth===393 && ${renderer}.innerHeight===852`);
    assert.deepEqual(await target.evaluate(`({width:${renderer}.innerWidth,height:${renderer}.innerHeight})`), {width:393,height:852}, 'the content renderer receives the chosen viewport');
    assert.deepEqual((await external()).viewport, {width:393,height:852}, 'Storybook itself receives the Mobile viewport');
    assert.equal(await target.evaluate(active + '===storyFrame'), true, 'resizing retains the warm Storybook document');
    assert.equal(requests.filter(url => url.startsWith('/iframe.html')).length, 1, 'resizing does not reload Storybook');
    await pick('storybook', 'secondary');
    await external({value:'Parked Story'});
    await target.evaluate(`window.ordinaryRenderer=document.querySelector('wb-canvas > iframe');location.hash='#feedback.workbench.ts~design'`);
    await until(`${canvas}.artboards.length===5`);
    assert.deepEqual(await target.evaluate(`${canvas}.artboards.map(b=>b.target.label)`), ['info','success','warning','danger','autopilot'].map(id=>'Feedback / '+id), 'authored artboard names are available before their renderers initialize');
    await until(`${canvas}.artboards.length===5 && ${canvas}.artboards.every(b=>b.target.artboard && b.view?.ready)`);
    assert.deepEqual(await target.evaluate("({rects:ordinaryRenderer.getClientRects().length,inert:ordinaryRenderer.inert,connected:ordinaryRenderer.isConnected})"), {rects:0,inert:true,connected:true}, 'the warm owner stays attached, hidden and inert');
    await pick('storybook', 'secondary');
    assert.equal(await target.evaluate("document.querySelector('wb-canvas > iframe')===ordinaryRenderer"), true);
    assert.equal(await target.evaluate(active + '===storyFrame'), true, 'returning from explicit artboards restores the same Storybook document');
    assert.equal((await external()).value, 'Parked Story');
    await pick('app');
    assert.equal(await target.evaluate(active + '===urlFrame'), true);
    assert.equal((await external()).value, 'Edited URL');
    await pick('');
    // Inactive frames cannot navigate the active canvas through the bridge.
    await external({ navigate: true }, 'storyFrame');
    assert.equal(await target.evaluate(active + '===reactFrame'), true);
    const changed = await target.evaluate(renderer + '.changes');
    await target.evaluate('document.getElementById("reload").click()');
    await until(renderer + '.changes > ' + changed);
    assert.equal(await target.evaluate(active + '!==reactFrame && ' + page + '.document.querySelector("#edit").value === "Original"'), true);
    assert.equal(requests.filter(url => url.startsWith('/iframe.html')).length, 1, 'Storybook boots once across lens switches and channel selection: ' + JSON.stringify(requests.filter(url => url.startsWith('/iframe.html'))));
    assert.equal(requests.filter(url => url === '/index.json').length, 2, 'each of the two titles is resolved once for both shells');
    assert.equal(requests.filter(url => url === '/page').length, 1, 'URL lens boots once');
    assert.equal(await target.evaluate(`${canvas}.artboards[0].id`), rendererId, 'ordinary navigation retains its owning renderer');
    await target.evaluate('location.hash="#other.md~docs"');
    await until(`${canvas}.artboards[0]?.view?.ready && ${canvas}.artboards[0].target.lens==='docs'`);
    assert.equal(await target.evaluate(`${mobile}.disabled`), true, 'the same Markdown page disables sizes in its Docs lens');
    await target.evaluate('location.hash="#other.md:default@mobile~storybook"');
    await until(`${canvas}.artboards[0]?.view?.ready && ${canvas}.artboards[0].target.lens==='storybook'`);
    assert.equal(await target.evaluate(`${mobile}.disabled`), false, 'returning from Docs restores Storybook size controls');
    assert.deepEqual(await target.evaluate(`${canvas}.artboards[0].size`), {width:393,height:852}, 'returning from Docs restores Mobile dimensions rather than its reading viewport');
    await until(`${renderer}.innerWidth===393 && ${renderer}.innerHeight===852`);
    assert.deepEqual((await external()).viewport, {width:393,height:852});
    await target.evaluate('document.querySelector("#lenses [data-lens=docs]").click()');
    await until(`${canvas}.artboards[0]?.view?.ready && ${canvas}.artboards[0].target.lens==='docs'`);
    await target.evaluate('document.querySelector("#lenses [data-lens=storybook]").click()');
    await until(`${canvas}.artboards[0]?.view?.ready && ${canvas}.artboards[0].target.lens==='storybook' && ${renderer}.innerWidth===393 && ${renderer}.innerHeight===852`);
    assert.equal(await target.evaluate(`${mobile}.getAttribute('aria-pressed')`), 'true', 'the actual lens controls restore the selected Mobile size');
    assert.deepEqual((await external()).viewport, {width:393,height:852}, 'the story viewport agrees after a toolbar Docs round trip');
    assert.equal(await target.evaluate(active + '===storyFrame'), true);
    await pick('app');
    await external({navigate:new URL('/other.md', running.url).href});
    await until(`location.hash.startsWith('#other.md') && ${canvas}.artboards[0]?.target.src==='other.md' && ${canvas}.artboards[0]?.view?.ready`);
    assert.equal(await target.evaluate('getComputedStyle(document.getElementById("artboards")).visibility'), 'visible', 'a preview link still updates the outer destination and reveals its content');
    await pick('');
    await pick('storybook', 'secondary');
    await target.evaluate(`window.retryStoryFrame=${active}`);
    await external({value:'Retained through failure'});
    catalogDelay = 700;
    await target.evaluate("location.hash='#broken.html:default~storybook'");
    await until('document.getElementById("crumbPage").textContent === "Broken"');
    await pick('storybook', 'secondary');
    await new Promise(resolve => setTimeout(resolve, 750));
    assert.equal((await loadingStatus()).hidden, true, 'a superseded lookup failure cannot replace the current ready page');
    catalogDelay = 0;
    await target.evaluate("location.hash='#broken.html:default~storybook'");
    await until('!document.getElementById("blank").hidden && !document.getElementById("blank").classList.contains("is-loading") && document.getElementById("blank").textContent.includes("Components/Missing")');
    const failure = await loadingStatus();
    assert.equal(failure.busy, 'false', 'a catalog failure settles the loading status');
    await new Promise(resolve => setTimeout(resolve, 550));
    assert.deepEqual(await loadingStatus(), failure, 'outgoing renderer polling cannot replace the failure with loading');
    includeMissing = true;
    await target.evaluate("location.hash='#broken.html~design'");
    await until(`${canvas}.artboards[0]?.target.src==='broken.html' && ${canvas}.artboards[0]?.view?.ready && ${canvas}.artboards[0]?.target.lens===null`);
    await target.evaluate("location.hash='#broken.html:default~storybook'");
    await until(`${canvas}.artboards[0]?.target.src==='broken.html' && ${canvas}.artboards[0]?.view?.ready && ${canvas}.artboards[0]?.target.lens==='storybook'`);
    assert.equal((await loadingStatus()).hidden, true, 'a successful retry clears the failure');
    assert.equal(await target.evaluate(active + '===retryStoryFrame'), true, 'retrying a failed title still reuses the warm Storybook session');
    assert.equal((await external()).value, 'Retained through failure');
    await target.evaluate("location.hash='#unknown.html'");
    await until('document.getElementById("blank").textContent === "Pick a page or a component to see it here."');
    assert.deepEqual(await loadingStatus(), {hidden:false,text:'Pick a page or a component to see it here.',loading:false,busy:'false'}, 'an empty selection clears loading without duplicating the placeholder');
    const labels = await target.evaluate('labelTimeline');
    assert.ok(labels.length > 0);
    for (const sample of labels) {
      if (sample.title.includes(' · Storybook · ') && sample.preset==='mobile')
        assert.deepEqual(sample.size, {width:393,height:852}, 'every visible Mobile Storybook frame agrees with its preset');
      assert.ok(sample.title.startsWith(sample.page+' · '), JSON.stringify(sample));
      for (const label of sample.labels) {
        assert.ok(label.startsWith(sample.page+' / ') || label.startsWith(sample.page+' · '), JSON.stringify(sample));
        if (sample.title.includes(' · Storybook · ')) assert.ok(label.startsWith(sample.page+' / '+sample.story+' · '), JSON.stringify(sample));
        assert.doesNotMatch(label, /\.workbench\.ts|\.html/, 'visible labels never fall back to source filenames');
      }
    }
    // Exercise the real editor iframe boundary: pending selection is followed
    // by a completion from the outer Workbench, after the story renders.
    fs.writeFileSync(path.join(root, 'editor.html'), `<!doctype html><iframe id="workbench" src="${running.url}#page.workbench.ts~docs"></iframe><script>window.reports=[];addEventListener('message',e=>{if(e.source===document.getElementById('workbench').contentWindow&&e.data?.type==='wb-here')reports.push(e.data)})</script>`);
    await target.navigate(new URL('/editor.html', running.url).href);
    await until("reports.some(r=>!r.pending && r.lens==='docs')");
    await target.evaluate("document.getElementById('workbench').contentWindow.location.hash='#page.workbench.ts:default~storybook'");
    await until("reports.some(r=>r.pending && r.lens==='storybook')");
    assert.equal(await target.evaluate("reports.some(r=>!r.pending && r.lens==='storybook')"), false);
    await until("reports.some(r=>!r.pending && r.lens==='storybook' && r.state==='default')");
    await target.evaluate("document.getElementById('workbench').contentWindow.location.hash='#page.workbench.ts:secondary~storybook'");
    const embedded = "document.getElementById('workbench').contentDocument.getElementById('artboards').state.artboards[0]";
    await until(`${embedded}?.target.state==='secondary' && !${embedded}.view?.ready`);
    assert.equal(await target.evaluate('getComputedStyle(document.getElementById("workbench").contentDocument.getElementById("artboards")).visibility'), 'hidden');
    await target.evaluate("document.getElementById('workbench').contentWindow.location.hash='#page.workbench.ts:default~storybook'");
    await until("reports.filter(r=>!r.pending && r.lens==='storybook' && r.state==='default').length===2");
    assert.equal(await target.evaluate("reports.some(r=>!r.pending && r.state==='secondary')"), false, 'a superseded story cannot acknowledge the current selection');
    await target.navigate(running.url + 'index.html#page.workbench.ts:default@laptop~storybook');
    await until("!!document.querySelector('wb-canvas > iframe')");
    assert.equal(await target.evaluate("getComputedStyle(document.querySelector('wb-canvas > iframe')).visibility"), 'hidden', 'cold startup cannot expose the nested Workbench UI');
    await until(`${canvas}.artboards[0]?.view?.ready`);
    assert.equal(await target.evaluate("getComputedStyle(document.querySelector('wb-canvas > iframe')).visibility"), 'visible');
    const beforeZoom = await target.evaluate("document.getElementById('artboards').scale");
    await external({key:'+'});
    await until(`document.getElementById('artboards').scale > ${beforeZoom}`);
    assert.deepEqual(failures, []);
    console.log('Passed: synchronized labels on every painted frame, retained sessions, one loading status through lookup/render, failure/retry, superseded errors, empty selection, capture gating, editor acknowledgements, hidden cold startup, inactive navigation isolation and Reload.');
  } finally {
    await browser.close();
    if (running) await running.close();
    await new Promise(resolve => app.close(resolve));
    fs.rmSync(root, { recursive: true, force: true });
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
