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
  const requests = [];
  const app = http.createServer((req, res) => {
    requests.push(req.url);
    res.setHeader('Content-Type', 'text/html');
    if (req.url === '/index.json') {
      res.setHeader('Content-Type', 'application/json');
      res.end(JSON.stringify({ entries: Object.fromEntries(['Page', 'Other'].flatMap(title => ['default', 'secondary'].map(state => [
        title.toLowerCase() + '--' + state, { id: title.toLowerCase() + '--' + state, type: 'story', title: 'Components/' + title, name: state },
      ]))) }));
      return;
    }
    res.end(`<!doctype html><html><body><h1>Acme lens</h1><input id="edit" value="Original">
      <details id="menu"><summary>Menu</summary>Open menu</details><div style="height:2500px"></div>
      <script>
      function announce(type,args){parent.postMessage(JSON.stringify({key:'storybook-channel',event:{type,args}}),'*')}
      function render(id){announce('currentStoryWasSet',[{storyId:id}]);setTimeout(()=>{window.story=id;announce('storyRendered',[id])},250)}
      addEventListener('message',event=>{let data=event.data;try{if(typeof data==='string')data=JSON.parse(data)}catch{return}
        if(data?.type==='fixture-command'){
          if(data.value!==undefined)document.querySelector('#edit').value=data.value;
          if(data.menu!==undefined)document.querySelector('#menu').open=data.menu;
          if(data.navigate)parent.postMessage({type:'wb-preview-navigate',href:'/missing'},'*');
          parent.postMessage({type:'fixture-state',token:data.token,value:document.querySelector('#edit').value,
            menu:document.querySelector('#menu').open,story:window.story},'*');return;}
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
        src: other.html
        implementations:
          storybook: Components/Other
      - label: Feedback
        src: feedback.workbench.ts
`);
  fs.writeFileSync(path.join(root, 'other.html'), '<!doctype html><body>Other authored page');
  fs.writeFileSync(path.join(root, 'feedback.workbench.ts'), `import {definePreview} from '@canonic2/workbench';export default definePreview({id:'feedback',adapter:'html',source:{entry:'./feedback.ts'},artboards:${JSON.stringify(Object.fromEntries(['info','success','warning','danger','autopilot'].map(id => [id,{label:id,width:320,height:120}])))} });`);
  fs.writeFileSync(path.join(root, 'feedback.ts'), `export default function(canvas,context){canvas.textContent=context.artboard;}`);
  fs.writeFileSync(path.join(root, 'page.md'), '# Acme documentation\n\nPage documentation.');
  try {
    running = await server.start({ root, capture: { warm() {}, prepare() {}, preparePage() {}, close() {} } });
    console.log('workbench on ' + running.url);
    await browser.launch();
    const target = await browser.createTarget();
    const failures = [];
    target.on('Runtime.exceptionThrown', event => failures.push(event.exceptionDetails.text));
    await target.send('Emulation.setDeviceMetricsOverride', { width: 1900, height: 1100, deviceScaleFactor: 1, mobile: false });
    await target.send('Page.addScriptToEvaluateOnNewDocument', { source:
      'window.changes=0;window.fixtureReports=[];addEventListener("wb-frame-change",()=>changes++);addEventListener("message",event=>{if(event.data?.type==="fixture-state")fixtureReports.push(event.data)});' });
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
      }
      await until(`${canvas}.artboards[0]?.view?.ready && ${canvas}.artboards[0].target.lens===${JSON.stringify(lens || null)}${lens === 'storybook' ? ` && ${canvas}.artboards[0].target.state===${JSON.stringify(state)}` : ''}`);
      console.log(JSON.stringify({ lens: lens || 'workbench', state, ms: Math.round(performance.now() - started) }));
    }
    const canvas = 'document.getElementById("artboards").state';
    const renderer = 'document.querySelector("wb-canvas > iframe").contentWindow';
    const active = renderer + '.document.querySelector("iframe.is-active")';
    const page = active + '.contentWindow';
    let token = 0;
    async function external(command = {}, which = active) {
      command = { ...command, token: ++token, type: 'fixture-command' };
      await target.evaluate(which + '.contentWindow.postMessage(' + JSON.stringify(command) + ',"*")');
      await until(renderer + '.fixtureReports.some(report=>report.token===' + token + ')');
      return target.evaluate(renderer + '.fixtureReports.find(report=>report.token===' + token + ')');
    }
    // Retained preview sessions belong to each isolated artboard renderer.
    await target.navigate(running.url + 'index.html#page.workbench.ts:default@laptop~design');
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
    await pick('docs');
    await pick('storybook', 'secondary');
    assert.equal(await target.evaluate(active + '===storyFrame'), true, 'docs navigation retains the Storybook session');
    await target.evaluate('location.hash="#other.html:default@laptop~storybook"');
    await until(`${canvas}.artboards[0]?.target.src==='other.html' && ${canvas}.artboards[0]?.view?.ready`);
    assert.equal(await target.evaluate(active + '===storyFrame'), true, 'a different mapped page uses the same Storybook document');
    assert.equal((await external()).story, 'other--default');
    await pick('storybook', 'secondary');
    await external({value:'Parked Story'});
    await target.evaluate(`window.ordinaryRenderer=document.querySelector('wb-canvas > iframe');location.hash='#feedback.workbench.ts~design'`);
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
    assert.equal(requests.filter(url => url.startsWith('/iframe.html')).length, 1, 'Storybook boots once across lens switches and channel selection');
    assert.equal(requests.filter(url => url === '/index.json').length, 2, 'each of the two titles is resolved once for both shells');
    assert.equal(requests.filter(url => url === '/page').length, 1, 'URL lens boots once');
    assert.equal(await target.evaluate(`${canvas}.artboards[0].id`), rendererId, 'ordinary navigation retains its owning renderer');
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
    await target.evaluate("document.getElementById('workbench').contentWindow.location.hash='#page.workbench.ts:default~storybook'");
    await until("reports.filter(r=>!r.pending && r.lens==='storybook' && r.state==='default').length===2");
    assert.equal(await target.evaluate("reports.some(r=>!r.pending && r.state==='secondary')"), false, 'a superseded story cannot acknowledge the current selection');
    await target.navigate(running.url + 'index.html#page.workbench.ts:default@laptop~storybook');
    await until("!!document.querySelector('wb-canvas > iframe')");
    assert.equal(await target.evaluate("getComputedStyle(document.querySelector('wb-canvas > iframe')).visibility"), 'hidden', 'cold startup cannot expose the nested Workbench UI');
    await until(`${canvas}.artboards[0]?.view?.ready`);
    assert.equal(await target.evaluate("getComputedStyle(document.querySelector('wb-canvas > iframe')).visibility"), 'visible');
    assert.deepEqual(failures, []);
    console.log('Passed: retained React/Storybook/URL sessions, delayed readiness, editor acknowledgements, superseded picks, hidden cold startup, inactive navigation isolation and Reload.');
  } finally {
    await browser.close();
    if (running) await running.close();
    await new Promise(resolve => app.close(resolve));
    fs.rmSync(root, { recursive: true, force: true });
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
