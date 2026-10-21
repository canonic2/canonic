import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { createHash } from 'node:crypto';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { createUiAssets } from '../../../../../scripts/ui-browser-fixture.cjs';

const { Browser, findChrome } = createRequire(import.meta.url)('../../../../../scripts/chrome.cjs');
const root = path.resolve(fileURLToPath(new URL('../../../../../', import.meta.url)));

void test('the shipped highlight.js modules match their pinned local assets', async () => {
  const vendor = path.join(root, 'ui/src/vendor/highlight.js');
  const manifest = JSON.parse(await readFile(path.join(vendor, 'manifest.json'), 'utf8'));
  assert.equal(manifest.version, '11.11.1');
  for (const [file, hash] of Object.entries(manifest.files))
    assert.equal(
      createHash('sha256')
        .update(await readFile(path.join(vendor, file)))
        .digest('hex'),
      hash,
      file,
    );
  assert.match(await readFile(path.join(vendor, 'LICENSE'), 'utf8'), /Redistribution/);
});

void test(
  'local worker colouring follows load, preserves every character, and settles captures',
  {
    skip: !findChrome() && 'Chrome is required',
  },
  async (t) => {
    const browser = new Browser();
    const assets = createUiAssets(root);
    const held = new Map();
    let holdWorker = true;
    const requests = [];
    const text = 'const value = "<Acme> λ"; // note ffi =>';
    const server = createServer((req, res) => {
      void (async () => {
        requests.push(req.url);
        if (holdWorker && req.url.endsWith('/highlight-worker.js')) {
          held.set(req.url, res);
        } else if (req.url.startsWith('/_workbench/')) {
          const asset = await assets(req.url);
          res.writeHead(asset.status, { 'Content-Type': asset.type });
          res.end(asset.body);
        } else if (req.url.startsWith('/source')) {
          res.setHeader('Content-Type', 'application/json');
          res.end(JSON.stringify({ text, file: 'acme.ts' }));
        } else {
          res.setHeader('Content-Type', 'text/html; charset=utf-8');
          res.end(`<!doctype html><link rel="stylesheet" href="/_workbench/ui/src/modules/docs/page/docs-page.css">
          <main class="wb-docs"><h1>Acme</h1><pre class="wb-docs-code"><code data-wb-code="ts">const value = &quot;&lt;Acme&gt; λ&quot;; // note ffi =&gt;</code></pre>
          <figure data-wb-example="basic"><button data-wb-show-code>Show code</button><button data-wb-copy-code>Copy code</button><div class="wb-docs-example-code" data-wb-example-code hidden></div></figure></main>
          <script>window.__workbenchDocs={page:'acme.md',highlightUrl:'/colour',sourceUrl:'/source?example=',bundle:null};
          window.workerLoadStates=[];const NativeWorker=Worker;window.Worker=class extends NativeWorker{constructor(...args){window.workerLoadStates.push(document.readyState);super(...args);}};</script>
          <script type="module" src="/_workbench/ui/src/modules/docs/page/docs-page.js"></script>`);
        }
      })().catch((error) => {
        res.statusCode = 500;
        res.end(String(error));
      });
    });
    t.after(async () => {
      for (const response of held.values()) response.end('');
      await browser.close();
      await new Promise((resolve) => server.close(resolve));
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    await browser.launch();
    const target = await browser.createTarget();
    await target.navigate(`http://127.0.0.1:${server.address().port}/`);
    const before = await target.evaluate(`(async()=>{
    const wait=async(test)=>{const end=performance.now()+3000;while(!test()){if(performance.now()>end)throw Error('Fixture timed out');await new Promise(r=>setTimeout(r,10));}};
    window.waitForFixture=wait;
    window.characterBoxes=element=>{const result=[],walk=document.createTreeWalker(element,4),range=document.createRange();let node;while(node=walk.nextNode())for(let i=0;i<node.length;i++){range.setStart(node,i);range.setEnd(node,i+1);result.push(range.getBoundingClientRect().toJSON());}return result;};
    await wait(()=>window.__workbenchReady===true);
    const node=document.querySelector('code[data-wb-code]');
    window.beforeNode=node.firstChild;window.beforeBoxes=characterBoxes(node);
    document.querySelector('[data-wb-show-code]').click();
    await wait(()=>document.querySelector('[data-wb-example-code]').dataset.loaded==='true');
    window.colourSettled=false;void wbDocsPage.whenHighlighted().then(()=>window.colourSettled=true);
    return {ready:window.__workbenchReady,text:node.textContent,spans:node.children.length,source:document.querySelector('[data-wb-example-code] code').textContent,load:document.readyState};
  })()`);
    assert.deepEqual(before, { ready: true, text, spans: 0, source: text, load: 'complete' });
    const deadline = Date.now() + 3000;
    while (
      ![...held.keys()].some((url) => url.endsWith('/highlight-worker.js')) &&
      Date.now() < deadline
    )
      await new Promise((resolve) => setTimeout(resolve, 10));
    assert.equal(held.size, 1, 'one document-owned worker serves fences and Show code');
    assert.deepEqual(await target.evaluate('window.workerLoadStates'), ['complete']);
    assert.equal(
      await target.evaluate('window.colourSettled'),
      false,
      'captures wait while colours are pending',
    );
    holdWorker = false;
    for (const [url, response] of held) {
      const asset = await assets(url);
      response.writeHead(asset.status, { 'Content-Type': asset.type });
      response.end(asset.body);
    }
    held.clear();
    const after = await target.evaluate(`(async()=>{
    await waitForFixture(()=>window.colourSettled);
    const code=document.querySelector('code[data-wb-code]');
    Object.defineProperty(navigator,'clipboard',{configurable:true,value:{writeText:async text=>{window.copiedCode=text;}}});
    document.querySelector('[data-wb-copy-code]').click();await waitForFixture(()=>window.copiedCode!==undefined);
    const keyword=[...CSS.highlights].find(([name])=>name.endsWith('-keyword'));
    const ranges=keyword?[...keyword[1]]:[];
    return {text:code.textContent,spans:code.children.length,sameNode:code.firstChild===beforeNode,
      geometry:JSON.stringify(beforeBoxes)===JSON.stringify(characterBoxes(code)),
      keyword:ranges.some(range=>range.toString()==='const'&&range.startContainer===code.firstChild),
      source:ranges.some(range=>range.toString()==='const'&&range.startContainer.parentElement.closest('[data-wb-example-code]')),
      colour:keyword&&getComputedStyle(code,'::highlight('+keyword[0]+')').color,copy:window.copiedCode};
  })()`);
    assert.deepEqual(after, {
      text,
      spans: 0,
      sameNode: true,
      geometry: true,
      keyword: true,
      source: true,
      colour: 'rgb(207, 34, 46)',
      copy: text,
    });
    assert.equal(requests.filter((url) => url.endsWith('/highlight-worker.js')).length, 1);
    assert.equal(
      requests.filter((url) => url.startsWith('/source')).length,
      1,
      'colouring reuses source fetched for readability',
    );
    assert.equal(
      requests.includes('/colour'),
      false,
      'live highlighting performs no server tokenization request',
    );
    assert.ok(
      requests.every((url) => url.startsWith('/')),
      'all assets are local',
    );

    const lifecycle = await target.evaluate(`(async()=>{
    const {createHighlighting}=await import('/_workbench/ui/src/modules/docs/page/highlighting.js');
    const node=document.createElement('code');document.body.append(node);node.textContent='original';
    const originalRanges=[...CSS.highlights].reduce((n,[,value])=>n+value.size,0);
    let answers=[];
    const highlighter=createHighlighting({frame:()=>Promise.resolve(),colour:()=>new Promise(resolve=>answers.push(resolve))});
    highlighter.source(node,'ts');await waitForFixture(()=>answers.length===1);
    node.textContent='edited';answers[0]([{text:'original',html:'<span class="hljs-keyword">original</span>'}]);
    await highlighter.settled();
    const fresh=node.textContent==='edited'&&node.children.length===0;
    highlighter.source(node,'ts');await waitForFixture(()=>answers.length===2);
    highlighter.dispose();await highlighter.settled();
    answers[1]([{text:'edited',html:'<span class="hljs-keyword">edited</span>'}]);await Promise.resolve();await Promise.resolve();
    const disposed=node.children.length===0&&[...CSS.highlights].reduce((n,[,value])=>n+value.size,0)===originalRanges;
    const failed=createHighlighting({frame:()=>Promise.resolve(),colour:async()=>{throw Error('Failed');}});
    failed.source(node,'ts');await failed.settled();failed.dispose();
    const failedPlain=node.textContent==='edited'&&node.children.length===0;
    const timeout=createHighlighting({frame:()=>Promise.resolve(),colour:()=>new Promise(()=>{})});
    timeout.source(node,'ts');const start=performance.now();await timeout.settled();timeout.dispose();
    return {fresh,disposed,failed:failedPlain,timeout:performance.now()-start>=4900&&node.textContent==='edited'};
  })()`);
    assert.deepEqual(lifecycle, { fresh: true, disposed: true, failed: true, timeout: true });

    const coverage = await target.evaluate(`(async()=>{
    const {createWorkerColouring}=await import('/_workbench/ui/src/modules/docs/page/highlight-worker-client.js');
    const client=createWorkerColouring();
    try {
      const snippets=[['sh','# Acme note'],['css','/* Acme */'],['diff','-old\\n+new'],['js','const value = 42;'],['jsx','const value = <button/>;'],['json','{"count":42}'],['md','# Acme'],['scss','$accent: red;'],[' TS ','const value: number = 42;'],['tsx','const value = <button/>;'],['html','<button title="Acme"/>'],['svg','<card title="Acme"/>'],['yml','count: 42'],['unknown-acme','<img src=x onerror="throw 1"> λ &'],['','<script>throw 1</script>']];
      const blocks=snippets.map(([language,text])=>({language,text}));
      const result=await client.colour(blocks,{signal:new AbortController().signal});
      const exact=result.every((block,index)=>{const t=document.createElement('template');t.innerHTML=block.html;return t.content.textContent===blocks[index].text;});
      const known=result.slice(0,13).every(block=>block.html.includes('hljs-'));
      const unknown=result.slice(13).every(block=>!block.html.includes('<'));
      const scss=result[7].html.includes('hljs-variable');
      const controller=new AbortController();const cancelled=client.colour(blocks,{signal:controller.signal}).catch(error=>error.name);controller.abort();
      const abort=await cancelled;
      const again=await client.colour([{text:'const value=42',language:'ts'}],{signal:new AbortController().signal});
      return {exact,known,unknown,scss,abort,restart:again[0].html.includes('hljs-keyword')};
    } finally {client.dispose();}
  })()`);
    assert.deepEqual(coverage, {
      exact: true,
      known: true,
      unknown: true,
      scss: true,
      abort: 'AbortError',
      restart: true,
    });

    const failure = await target.evaluate(`(async()=>{
    const {createWorkerColouring}=await import('/_workbench/ui/src/modules/docs/page/highlight-worker-client.js');
    const url=URL.createObjectURL(new Blob(['throw Error("Acme worker failure")'],{type:'text/javascript'}));
    let attempts=0;
    const client=createWorkerColouring({createWorker:()=>++attempts===1?new Worker(url):new Worker(new URL('/_workbench/ui/src/modules/docs/page/highlight-worker.js',location.href),{type:'module'})});
    try {
      const blocks=[{text:'const value=42',language:'ts'}];
      const error=await client.colour(blocks,{signal:new AbortController().signal}).then(()=>'',error=>error.message);
      const result=await client.colour(blocks,{signal:new AbortController().signal});
      const unknown=await client.colour([{text:'<Acme>',language:'constructor'},...blocks],{signal:new AbortController().signal});
      return {observed:error.includes('Acme worker failure'),restarted:result[0].html.includes('hljs-keyword'),isolated:unknown[0].html==='&lt;Acme&gt;'&&unknown[1].html.includes('hljs-keyword')};
    } finally {client.dispose();URL.revokeObjectURL(url);}
  })()`);
    assert.deepEqual(failure, { observed: true, restarted: true, isolated: true });

    const mirror = await target.evaluate(`(async()=>{
    await import('/_workbench/ui/legacy/dom-mirror.js');
    const {createCodeColours}=await import('/_workbench/ui/src/modules/docs/page/code-colours.js');
    const makeFrame=()=>new Promise(resolve=>{const frame=document.createElement('iframe');frame.onload=()=>resolve(frame);frame.srcdoc='<!doctype html><style>:root{--wb-docs-syntax-keyword:rgb(207,34,46)}</style><code>const value = 42;</code>';document.body.append(frame);});
    const source=await makeFrame(),receiver=await makeFrame();
    const paint=createCodeColours(source.contentDocument),mirror=wbDOMMirror.create(source.contentDocument);
    const apply=async snapshot=>wbDOMMirror.apply(receiver.contentDocument,JSON.parse(JSON.stringify(snapshot)));
    try {
      const initial=mirror.read();await apply(initial);mirror.acknowledge(initial);
      const code=source.contentDocument.querySelector('code');
      paint.apply(code,code.textContent,'<span class="hljs-keyword">const</span> value = 42;');
      const coloured=mirror.read();await apply(coloured);mirror.acknowledge(coloured);
      const view=receiver.contentWindow,node=view.document.querySelector('code'),entry=[...view.CSS.highlights][0],originalNode=node.firstChild;
      const transferred=!!entry&&[...entry[1]][0].toString()==='const'&&view.getComputedStyle(node,'::highlight('+entry[0]+')').color==='rgb(207, 34, 46)'&&node.children.length===0;
      const unchanged=mirror.read();await apply(unchanged);
      const retained=node.firstChild===originalNode&&view.CSS.highlights.size===1;
      paint.reset();const cleared=mirror.read();await apply(cleared);
      return {transferred,retained,cleared:view.CSS.highlights.size===0,revisions:coloured.revision!==initial.revision&&cleared.revision!==coloured.revision,text:node.textContent};
    } finally {mirror.stop();paint.dispose();source.remove();receiver.remove();}
  })()`);
    assert.deepEqual(mirror, {
      transferred: true,
      retained: true,
      cleared: true,
      revisions: true,
      text: 'const value = 42;',
    });
  },
);
