/* Real attached-frame retention check. Chrome is only the test driver. */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const http = require('node:http');
const { performance } = require('node:perf_hooks');
const server = require('../server');
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
      res.end(JSON.stringify({ entries: Object.fromEntries(['default', 'secondary'].map(state => [
        'page--' + state, { id: 'page--' + state, type: 'story', title: 'Components/Page', name: state },
      ])) }));
      return;
    }
    res.end(`<!doctype html><html><body><h1>Acme lens</h1><input id="edit" value="Original">
      <details id="menu"><summary>Menu</summary>Open menu</details><div style="height:2500px"></div>
      <script>
      function announce(type,args){parent.postMessage(JSON.stringify({key:'storybook-channel',event:{type,args}}),'*')}
      function render(id){window.story=id;announce('currentStoryWasSet',[{storyId:id}]);announce('storyRendered',[id])}
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
  fs.symlinkSync(path.resolve(__dirname, '../node_modules'), path.join(root, 'node_modules'), 'junction');
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
        implementations:
          storybook: Components/Page
          app: /page
`);
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
      throw new Error('Retained preview did not settle: ' + expression);
    }
    async function pick(lens, state = 'default') {
      const started = performance.now();
      const changed = await target.evaluate('changes');
      await target.evaluate('location.hash=' + JSON.stringify('page.workbench.ts:' + state + '@1512~' + (lens || 'design')));
      await until('changes > ' + changed);
      console.log(JSON.stringify({ lens: lens || 'workbench', state, ms: Math.round(performance.now() - started) }));
    }
    const active = 'document.querySelector("iframe.is-active")';
    const page = active + '.contentWindow';
    let token = 0;
    async function external(command = {}, which = active) {
      command = { ...command, token: ++token, type: 'fixture-command' };
      await target.evaluate(which + '.contentWindow.postMessage(' + JSON.stringify(command) + ',"*")');
      await until('fixtureReports.some(report=>report.token===' + token + ')');
      return target.evaluate('fixtureReports.find(report=>report.token===' + token + ')');
    }
    // Retained preview sessions belong to each isolated artboard renderer.
    await target.navigate(running.url + 'index.html#page.workbench.ts:default@1512');
    await until('changes > 0');
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
    await pick('storybook', 'secondary');
    assert.equal(await target.evaluate(active + '===storyFrame'), true);
    assert.equal((await external()).story, 'page--secondary');
    await pick('app');
    assert.equal(await target.evaluate(active + '===urlFrame'), true);
    assert.equal((await external()).value, 'Edited URL');
    await pick('');
    // Inactive frames cannot navigate the active canvas through the bridge.
    await external({ navigate: true }, 'storyFrame');
    assert.equal(await target.evaluate(active + '===reactFrame'), true);
    const changed = await target.evaluate('changes');
    await target.evaluate('document.getElementById("reload").click()');
    await until('changes > ' + changed);
    assert.equal(await target.evaluate(active + '!==reactFrame && ' + page + '.document.querySelector("#edit").value === "Original"'), true);
    assert.equal(requests.filter(url => url.startsWith('/iframe.html')).length, 1, 'Storybook boots once across lens switches and channel selection');
    assert.equal(requests.filter(url => url === '/page').length, 1, 'URL lens boots once');
    assert.deepEqual(failures, []);
    console.log('Passed: retained React state, form edits, menus, scroll, Storybook channel reuse, URL reuse, inactive navigation isolation and Reload.');
  } finally {
    await browser.close();
    if (running) await running.close();
    await new Promise(resolve => app.close(resolve));
    fs.rmSync(root, { recursive: true, force: true });
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
