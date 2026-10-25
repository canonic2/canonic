import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { createUiAssets } from '../../../../scripts/ui-browser-fixture.cjs';

const { Browser, findChrome } = createRequire(import.meta.url)('../../../../scripts/chrome.cjs');
const { build } = createRequire(import.meta.url)('esbuild');
const root = path.resolve(fileURLToPath(new URL('../../../../', import.meta.url)));

test(
  'warm Docs lifecycle, isolation and retained freshness in the browser',
  {
    skip: !findChrome() && 'Chrome is required',
  },
  async (t) => {
    const browser = new Browser();
    const assets = createUiAssets(root);
    const revisions = new Map();
    const held = new Set();
    const requests = [];
    let holdRevisions = false;
    let failRevisions = false;
    let failBundles = false;
    let mismatchOnce = false;
    let moduleId = 0;
    const revision = (page) => revisions.get(page) || 'v1';
    const reactBundle = (
      await build({
        stdin: {
          contents: `export { mount } from './ui/preview/react.js'; export { default as React } from 'react';`,
          resolveDir: root,
        },
        bundle: true,
        format: 'esm',
        platform: 'browser',
        define: { 'process.env.NODE_ENV': '"development"' },
        write: false,
      })
    ).outputFiles[0].text;
    const server = createServer((req, res) => {
      void (async () => {
        const url = new URL(req.url, 'http://example.com');
        requests.push(url.pathname + url.search);
        if (url.pathname === '/frame-owner') {
          res.setHeader('Content-Type', 'text/html');
          res.end(`<!doctype html><div id="surface"><div id="viewport">
            <iframe id="frame"></iframe><iframe id="buffer"></iframe></div></div>
            <script src="/_workbench/ui/legacy/preview-sessions.js"></script>
            <script src="/_workbench/ui/legacy/content-frames.js"></script>
            <script>
              window.__wbFailures=[];
              window.__wbFrames=wbContentFrames.create({
                surface:document.getElementById('surface'),viewport:document.getElementById('viewport'),
                frame:document.getElementById('frame'),buffer:document.getElementById('buffer'),
                changed(){},reveal(){},failed(error){__wbFailures.push(String(error));}
              });
              window.__wbShow=address=>__wbFrames.show({url:new URL(address,location.href).href,
                lens:{kind:'docs',key:'web'}});
            </script>`);
        } else if (url.pathname === '/_workbench/ui/src/app/docs/docs-host.html') {
          res.setHeader('Content-Type', 'text/html');
          res.end(await readFile(new URL('./docs-host.html', import.meta.url)));
        } else if (url.pathname === '/_workbench/docs/bundle') {
          const page = url.searchParams.get('page');
          res.setHeader('Content-Type', 'application/json');
          if (failBundles) {
            res.statusCode = 503;
            res.end(JSON.stringify({ error: 'Acme examples unavailable' }));
            return;
          }
          res.end(
            JSON.stringify({
              module:
                (page.startsWith('react-') ? '/react-examples.js' : '/examples.js') +
                '?page=' +
                page +
                '&load=' +
                ++moduleId,
              examples: ['basic'],
              revision: revision(page),
            }),
          );
        } else if (url.pathname === '/revision') {
          const respond = () => {
            res.setHeader('Content-Type', 'application/json');
            if (failRevisions) {
              res.statusCode = 503;
              res.end('{}');
              return;
            }
            res.end(JSON.stringify({ revision: revision(url.searchParams.get('page')) }));
          };
          if (holdRevisions) held.add(respond);
          else respond();
        } else if (url.pathname === '/source') {
          res.setHeader('Content-Type', 'application/json');
          res.end(
            JSON.stringify({ text: 'const acme = 1;', html: 'const acme = 1;', file: 'acme.js' }),
          );
        } else if (url.pathname === '/dependency.js') {
          res.setHeader('Content-Type', 'text/javascript');
          res.end('window.__wbDependencyLoads = (window.__wbDependencyLoads || 0) + 1;');
        } else if (url.pathname === '/react-adapter.js') {
          res.setHeader('Content-Type', 'text/javascript');
          res.end(
            reactBundle + '\nwindow.__wbReactRuntimeLoads=(window.__wbReactRuntimeLoads||0)+1;',
          );
        } else if (url.pathname === '/react-examples.js') {
          res.setHeader('Content-Type', 'text/javascript');
          res.end(`import { React, mount } from '/react-adapter.js';
          const name=${JSON.stringify(url.searchParams.get('page'))};
          function Selection(){
            React.useEffect(()=>{
              window.__wbPageListenerMounts=(window.__wbPageListenerMounts||0)+1;
              // Deliberately leaked by project code: the host still owns cleanup.
              document.addEventListener('selectionchange',()=>{
                window.__wbPageSelectionHits=(window.__wbPageSelectionHits||0)+1;
              });
            },[]);
            return React.createElement('input',{defaultValue:'Acme selection',onSelect(event){
              (window.__wbSelections||=[]).push({name,start:event.target.selectionStart,end:event.target.selectionEnd});
            }});
          }
          export const examples={basic:Selection};
          export const adapter={mount};`);
        } else if (url.pathname === '/examples.js') {
          res.setHeader('Content-Type', 'text/javascript');
          res.end(`import '/dependency.js';
          const name=${JSON.stringify(url.searchParams.get('page'))};
          export const examples={basic: name};
          export const environment={setup(context){
            (window.__wbScenarios ||= []).push(context.scenario);
            return ()=>{window.__wbEnvironmentStops=(window.__wbEnvironmentStops||0)+1;};
          }};
          export const adapter={async mount(stage, value, context){
            (window.__wbMounts ||= []).push({name, dirty:!!window.acmePage,
              portals:document.querySelectorAll('[data-acme-portal]').length,
              textStyles:document.querySelectorAll('style[data-acme-text]').length,
              body:document.body.className, html:document.documentElement.getAttribute('data-acme'),
              scenario:document.documentElement.getAttribute('data-wb-scenario')});
            const listener=()=>window.__wbHits=(window.__wbHits||0)+1;
            window.addEventListener('acme-event',listener);document.addEventListener('acme-event',listener);
            document.body.addEventListener('acme-event',listener);document.documentElement.addEventListener('acme-event',listener);
            window.acmePage=name;document.body.classList.add('acme-page');
            document.documentElement.setAttribute('data-acme',name);
            const portal=document.createElement('aside');portal.dataset.acmePortal=name;document.body.append(portal);
            const style=document.createElement('style');style.dataset.acmeText=name;style.textContent='body { --acme: red; }';document.head.append(style);
            if(!window.__wbLibrarySheet){const sheet=document.createElement('style');document.head.append(sheet);sheet.sheet.insertRule('body { --acme-library: 1; }');sheet.sheet.insertRule('html {scroll-behavior: smooth}');window.__wbLibrarySheet=sheet;}
            const timeout=setTimeout(()=>window.__wbLate=(window.__wbLate||0)+1,60000);
            const interval=setInterval(()=>window.__wbLate=(window.__wbLate||0)+1,60000);
            window.__wbLastTimers=[timeout,interval];
            stage.textContent=value;context.action('mounted',name);
            return async ()=>{
              window.__wbUnmounts=(window.__wbUnmounts||0)+1;stage.replaceChildren();
              if(name==='rejecting.md'||name==='rejecting-late.md'){
                parent.__wbCleanupCalls=(parent.__wbCleanupCalls||0)+1;
                if(name==='rejecting-late.md')await new Promise(resolve=>{parent.__wbReleaseCleanup=resolve;});
                throw Error('Acme unmount rejected');
              }
            };
          }};`);
        } else if (url.pathname.endsWith('.md')) {
          const page = url.pathname.slice(1);
          const htmlRevision = mismatchOnce ? 'old' : revision(page).split(':')[0];
          mismatchOnce = false;
          const info = '/_workbench/docs/bundle?page=' + encodeURIComponent(page) + '&lens=web';
          res.setHeader('Content-Type', 'text/html');
          res.end(`<!doctype html><title>${page}</title><main class="wb-docs">
          <h1>${page} ${revision(page)}</h1>
          <p data-wb-scenario-only="selected" id="selected-only">Selected</p>
          <p data-wb-scenario-only="default" id="default-only">Default</p>
          <figure data-wb-example="basic" data-status="pending"><div data-wb-example-stage></div>
          <button data-wb-show-code>Show code</button><button data-wb-copy-code>Copy code</button>
          <div data-wb-example-code hidden></div></figure><div style="height:2500px"></div>
          <h2 id="example-bottom">Bottom</h2></main>
          <script>window.__workbenchDocs=${JSON.stringify({
            page,
            lens: 'web',
            scenario: url.searchParams.get('scenario') || 'selected',
            sourceUrl: '/source?example=',
            revisionUrl: '/revision?page=' + page,
            revision: htmlRevision + ':',
            bundle: { info },
          })}</script>${
            url.searchParams.has('standalone')
              ? `<script type="module">
            import { start } from '/_workbench/ui/src/views/docs/docs-page.js';
            await start(window.__workbenchDocs);
          </script>`
              : ''
          }`);
        } else if (url.pathname === '/_workbench/preview-compat.js') {
          res.setHeader('Content-Type', 'text/javascript');
          res.end(
            'window.wbPreviewActions={configure(){}};\n' +
              (await readFile(new URL('../../../legacy/scenarios.js', import.meta.url), 'utf8')),
          );
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
      for (const respond of held) respond();
      await browser.close();
      await new Promise((resolve) => server.close(resolve));
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    await browser.launch();
    const origin = 'http://127.0.0.1:' + server.address().port;
    async function open() {
      const target = await browser.createTarget();
      await target.send('Page.addScriptToEvaluateOnNewDocument', {
        source: 'window.__wbTestLater = window.setTimeout.bind(window);',
      });
      await target.navigate(origin + '/_workbench/ui/src/app/docs/docs-host.html?lens=web');
      await target.evaluate(`(async()=>{
      window.__wbWait=async test=>{const end=performance.now()+5000;while(!test()){
        if(performance.now()>end)throw Error('Docs fixture timed out');await new Promise(r=>__wbTestLater(r,10));}};
      await __wbWait(()=>window.wbDocsHost);
      window.__wbLoad=async name=>{await wbDocsHost.load('/'+name+'.md?lens=web');await __wbWait(()=>window.__workbenchReady||window.__workbenchError);if(window.__workbenchError)throw Error(window.__workbenchError);};
    })()`);
      return target;
    }

    await t.test(
      'reassignment restores the baseline and retains dependencies and CSSOM sheets',
      async () => {
        const target = await open();
        const result = await target.evaluate(`(async()=>{
      await __wbLoad('one');const sheet=__wbLibrarySheet;scrollTo({top:300,behavior:'instant'});
      await __wbLoad('two');
      window.dispatchEvent(new Event('acme-event'));document.dispatchEvent(new Event('acme-event'));
      document.body.dispatchEvent(new Event('acme-event'));document.documentElement.dispatchEvent(new Event('acme-event'));
      return {mounts:__wbMounts,unmounts:__wbUnmounts,stops:__wbEnvironmentStops,hits:__wbHits,
        dependency:__wbDependencyLoads,sheet:sheet===__wbLibrarySheet&&sheet.isConnected,
        scroll:scrollY,scenarios:__wbScenarios,examples:wbDocsPage.visibleExamples()};
    })()`);
        assert.equal(result.mounts.length, 2);
        assert.deepEqual(result.mounts[1], {
          name: 'two.md',
          dirty: false,
          portals: 0,
          textStyles: 0,
          body: 'wb-docs-body',
          html: null,
          scenario: 'selected',
        });
        assert.equal(result.unmounts, 1);
        assert.equal(result.stops, 1);
        assert.equal(result.hits, 4);
        assert.equal(result.dependency, 1);
        assert.equal(result.sheet, true);
        assert.equal(
          result.scroll,
          0,
          'a new destination resets immediately despite smooth-scroll CSS',
        );
        assert.deepEqual(result.scenarios, ['selected', 'selected']);
        assert.deepEqual(result.examples, [{ id: 'basic', status: 'ready' }]);
        const pageRequests = requests.filter((url) =>
          url.startsWith('/_workbench/docs/bundle?page=two.md'),
        );
        assert.equal(
          pageRequests.length,
          1,
          'parallel metadata is consumed without a duplicate fetch',
        );
      },
    );

    await t.test(
      'real React onSelect survives reuse while page document listeners are released',
      async () => {
        const target = await open();
        const result = await target.evaluate(`(async()=>{
        const pages=[];
        let scopeValue,scopeError;
        for(const [index,name] of ['react-one','react-two','react-three'].entries()){
          await __wbLoad(name);
          await __wbWait(()=>window.__wbPageListenerMounts===index+1);
          if(index===0){
            scopeValue=__workbenchRetainRuntimeListeners(()=>{
              document.addEventListener('selectionchange',()=>{
                window.__wbRetainedSelectionHits=(window.__wbRetainedSelectionHits||0)+1;
              });
              return 42;
            });
            try{__workbenchRetainRuntimeListeners(()=>{throw Error('Acme initialization failed');});}
            catch(error){scopeError=error.message;}
          }
          const selections=window.__wbSelections?.length||0;
          const hits=window.__wbPageSelectionHits||0;
          const runtimeHits=window.__wbRetainedSelectionHits||0;
          const input=document.querySelector('[data-wb-example-stage] input');
          input.focus();input.setSelectionRange(1,5);
          document.dispatchEvent(new Event('selectionchange'));
          pages.push({selected:(window.__wbSelections?.length||0)-selections,
            pageListeners:(window.__wbPageSelectionHits||0)-hits,
            runtimeListeners:(window.__wbRetainedSelectionHits||0)-runtimeHits});
        }
        const hits=window.__wbPageSelectionHits;
        await wbDocsHost.reset();
        const runtimeHits=window.__wbRetainedSelectionHits;
        document.dispatchEvent(new Event('selectionchange'));
        return {pages,selections:window.__wbSelections,runtimeLoads:window.__wbReactRuntimeLoads,
          scopeValue,scopeError,scopeReleased:!window.__workbenchRetainRuntimeListeners,
          disposedRuntimeHits:window.__wbRetainedSelectionHits-runtimeHits,
          disposedPageHits:window.__wbPageSelectionHits-hits};
      })()`);
        assert.deepEqual(result.pages, [
          { selected: 1, pageListeners: 1, runtimeListeners: 1 },
          { selected: 1, pageListeners: 1, runtimeListeners: 1 },
          { selected: 1, pageListeners: 1, runtimeListeners: 1 },
        ]);
        assert.deepEqual(result.selections, [
          { name: 'react-one.md', start: 1, end: 5 },
          { name: 'react-two.md', start: 1, end: 5 },
          { name: 'react-three.md', start: 1, end: 5 },
        ]);
        assert.equal(result.runtimeLoads, 1);
        assert.equal(result.scopeValue, 42);
        assert.equal(result.scopeError, 'Acme initialization failed');
        assert.equal(result.scopeReleased, true);
        assert.equal(result.disposedRuntimeHits, 0);
        assert.equal(result.disposedPageHits, 0);
      },
    );

    await t.test(
      'rejecting example cleanup replaces the document and the canvas retries its destination',
      async () => {
        const target = await browser.createTarget();
        await target.navigate(origin + '/frame-owner');
        const result = await target.evaluate(`(async()=>{
          const wait=async test=>{const end=performance.now()+5000;while(!test()){
            if(performance.now()>end)throw Error('Canvas recovery fixture timed out');
            await new Promise(resolve=>setTimeout(resolve,10));
          }};
          let originalFrame,originalDocument;
          for(const name of ['rejecting','two','three','four']){
            __wbShow('/'+name+'.md?lens=web');
            await wait(()=>__wbFrames.ready()||__wbFailures.length);
            if(__wbFailures.length)throw Error(__wbFailures.join('; '));
            if(!originalFrame){originalFrame=__wbFrames.frame();originalDocument=originalFrame.contentDocument;}
          }
          __wbShow('/replacement.md?lens=web&scenario=default&actions=on#example-bottom');
          await wait(()=>__wbFrames.ready()||__wbFailures.length);
          const frame=__wbFrames.frame();const content=frame.contentWindow;
          return {failures:__wbFailures,ready:__wbFrames.ready(),cleanupCalls:__wbCleanupCalls,
            sameFrame:frame===originalFrame,freshDocument:frame.contentDocument!==originalDocument,
            url:content.location.pathname+content.location.search+content.location.hash,
            mounts:content.__wbMounts};
        })()`);
        assert.deepEqual(result.failures, []);
        assert.equal(result.ready, true);
        assert.equal(result.cleanupCalls, 1);
        assert.equal(result.sameFrame, true, 'the pool frame owns the retry');
        assert.equal(result.freshDocument, true, 'failed cleanup discards the poisoned document');
        assert.equal(
          result.url,
          '/replacement.md?lens=web&scenario=default&actions=on#example-bottom',
        );
        assert.equal(result.mounts.length, 1, 'the new document mounts only the requested page');
        assert.equal(result.mounts[0].name, 'replacement.md');
        assert.equal(result.mounts[0].dirty, false);
        assert.equal(result.mounts[0].portals, 0);
        await target.evaluate('__wbFrames.close()');
      },
    );

    await t.test(
      'late rejecting cleanup cannot replace the newer destination after cancellation',
      async () => {
        const target = await browser.createTarget();
        await target.navigate(origin + '/frame-owner');
        const result = await target.evaluate(`(async()=>{
          const wait=async test=>{const end=performance.now()+5000;while(!test()){
            if(performance.now()>end)throw Error('Canvas supersession fixture timed out');
            await new Promise(resolve=>setTimeout(resolve,10));
          }};
          let abandoned;
          for(const name of ['rejecting-late','two','three','four']){
            __wbShow('/'+name+'.md?lens=web');
            await wait(()=>__wbFrames.ready()||__wbFailures.length);
            if(__wbFailures.length)throw Error(__wbFailures.join('; '));
            abandoned ||= __wbFrames.frame();
          }
          __wbShow('/superseded.md?lens=web');
          await wait(()=>window.__wbReleaseCleanup);
          __wbShow('/newest.md?lens=web');
          await wait(()=>__wbFrames.ready()||__wbFailures.length);
          const latest=__wbFrames.frame().contentDocument;
          __wbReleaseCleanup();
          await wait(()=>!abandoned.isConnected);
          return {failures:__wbFailures,ready:__wbFrames.ready(),cleanupCalls:__wbCleanupCalls,
            sameDocument:__wbFrames.frame().contentDocument===latest,
            page:__wbFrames.frame().contentWindow.location.pathname};
        })()`);
        assert.deepEqual(result.failures, []);
        assert.equal(result.ready, true);
        assert.equal(result.cleanupCalls, 1);
        assert.equal(result.sameDocument, true);
        assert.equal(result.page, '/newest.md');
        await target.evaluate('__wbFrames.close()');
      },
    );

    await t.test(
      'second start mounts fresh, stop is repeatable and releases page work',
      async () => {
        const target = await open();
        const result = await target.evaluate(`(async()=>{
      await __wbLoad('one');
      const page=await import('/_workbench/ui/src/views/docs/docs-page.js');
      await page.start({...__workbenchDocs,bundle:{module:'/examples.js?page=again&load=direct'}});
      const actions=[];const ready=e=>actions.push(e.detail.actions);window.addEventListener('workbench:ready',ready);
      await page.start({...__workbenchDocs,bundle:{module:'/examples.js?page=last&load=direct'}});
      await page.stop();await page.stop();window.dispatchEvent(new Event('acme-event'));
      return {mounts:__wbMounts.length,unmounts:__wbUnmounts,stops:__wbEnvironmentStops,
        ready:__workbenchReady,actions:actions.map(a=>a.length),connection:!!window.__wbPushSource};
    })()`);
        assert.deepEqual(result, {
          mounts: 3,
          unmounts: 3,
          stops: 3,
          ready: false,
          actions: [1],
          connection: true,
        });
      },
    );

    await t.test(
      'retained resume is immediate, changed revisions preserve mounts, tools and scroll',
      async () => {
        const target = await open();
        await target.evaluate(
          `(async()=>{await __wbLoad('one');document.querySelector('[data-wb-show-code]').click();await __wbWait(()=>document.querySelector('[data-wb-example-code]').dataset.loaded);scrollTo({top:300,behavior:'instant'});})()`,
        );
        holdRevisions = true;
        const resumed = await target.evaluate(
          `(async()=>{await wbDocsHost.resume('/one.md?lens=web&wbTrace=return');return {ready:__workbenchReady,mounts:__wbMounts.length,scroll:scrollY};})()`,
        );
        assert.deepEqual(resumed, { ready: true, mounts: 1, scroll: 300 });
        revisions.set('one.md', 'v2');
        holdRevisions = false;
        // A held server response is deliberately completed after resume returned.
        await new Promise((resolve) => setTimeout(resolve, 30));
        for (const respond of held) respond();
        held.clear();
        const changed = await target.evaluate(`(async()=>{
      await __wbWait(()=>document.querySelector('h1').textContent.includes('v2'));
      await __wbWait(()=>document.querySelector('[data-wb-example-code]').dataset.loaded);
      return {mounts:__wbMounts.length,scroll:scrollY,open:!document.querySelector('[data-wb-example-code]').hidden,
        scenario:document.documentElement.getAttribute('data-wb-scenario'),conditional:!document.getElementById('default-only')&&!!document.getElementById('selected-only')};
    })()`);
        assert.deepEqual(changed, {
          mounts: 1,
          scroll: 300,
          open: true,
          scenario: 'selected',
          conditional: true,
        });
        revisions.clear();
      },
    );

    await t.test(
      'standalone example revisions reload when session storage is blocked',
      async () => {
        const target = await browser.createTarget();
        await target.send('Page.addScriptToEvaluateOnNewDocument', {
          source: `Object.defineProperty(window,'sessionStorage',{get(){throw new DOMException('Storage blocked','SecurityError');}});`,
        });
        revisions.set('standalone.md', 'v1:examples-v1');
        await target.navigate(origin + '/standalone.md?standalone=1');
        const wait = async (expression) => {
          for (let attempt = 0; attempt < 250; attempt++) {
            if (await target.evaluate(expression)) return;
            await new Promise((resolve) => setTimeout(resolve, 20));
          }
          assert.fail('Standalone page did not settle: ' + expression);
        };
        await wait('!!window.__workbenchReady');
        const before = requests.filter((url) => url === '/standalone.md?standalone=1').length;
        revisions.set('standalone.md', 'v1:examples-v2');
        await target.evaluate('void wbDocsPage.checkRevision(); true');
        await wait(
          '!!window.__workbenchReady && document.querySelector("h1")?.textContent.includes("examples-v2")',
        );
        assert.equal(
          requests.filter((url) => url === '/standalone.md?standalone=1').length,
          before + 1,
        );
        revisions.clear();
      },
    );

    await t.test(
      'late revision results and queued loads cannot overwrite a new destination',
      async () => {
        const target = await open();
        await target.evaluate(`__wbLoad('one')`);
        holdRevisions = true;
        await target.evaluate(`wbDocsHost.resume('/one.md?lens=web')`);
        await new Promise((resolve) => setTimeout(resolve, 30));
        await target.evaluate(`__wbLoad('two')`);
        revisions.set('one.md', 'obsolete');
        holdRevisions = false;
        for (const respond of held) respond();
        held.clear();
        const result = await target.evaluate(`(async()=>{
      await Promise.all([wbDocsHost.load('/old.md?lens=web'),wbDocsHost.load('/new.md?lens=web')]);
      await __wbWait(()=>__workbenchReady);
      return {title:document.querySelector('h1').textContent,mounts:__wbMounts.map(m=>m.name)};
    })()`);
        assert.equal(result.title, 'new.md v1');
        assert.deepEqual(result.mounts, ['one.md', 'two.md', 'new.md']);
        revisions.clear();
      },
    );

    await t.test(
      'a failed revision check leaves content usable and an example revision refreshes with scroll kept',
      async () => {
        const target = await open();
        await target.evaluate(
          `(async()=>{await __wbLoad('one');scrollTo({top:300,behavior:'instant'});})()`,
        );
        failRevisions = true;
        assert.deepEqual(
          await target.evaluate(
            `(async()=>{await wbDocsPage.checkRevision();return {ready:__workbenchReady,mounts:__wbMounts.length};})()`,
          ),
          { ready: true, mounts: 1 },
        );
        failRevisions = false;
        revisions.set('one.md', 'v1:examples-v2');
        const changed = await target.evaluate(`(async()=>{
        await wbDocsHost.resume('/one.md?lens=web');
        await __wbWait(()=>__workbenchReady&&__wbMounts.length===2);
        return {scroll:scrollY,stops:__wbUnmounts,title:document.querySelector('h1').textContent};
      })()`);
        assert.deepEqual(changed, { scroll: 300, stops: 1, title: 'one.md v1:examples-v2' });
        revisions.clear();
      },
    );

    await t.test(
      'parallel responses straddling a save retry, and unavailable metadata still shows prose',
      async () => {
        const target = await open();
        mismatchOnce = true;
        await target.evaluate(`__wbLoad('one')`);
        assert.equal(
          await target.evaluate(`document.querySelector('h1').textContent`),
          'one.md v1',
        );
        failBundles = true;
        const result = await target.evaluate(`(async()=>{
        await __wbLoad('two');return {ready:__workbenchReady,title:document.querySelector('h1').textContent,
          status:document.querySelector('[data-wb-example]').dataset.status,
          note:document.querySelector('[data-wb-example]').textContent};
      })()`);
        assert.deepEqual(result, {
          ready: true,
          title: 'two.md v1',
          status: 'unavailable',
          note: 'Acme examples unavailable',
        });
        failBundles = false;
      },
    );

    await t.test(
      'invalid destinations are refused and disposal prevents another mount',
      async () => {
        const target = await open();
        const result = await target.evaluate(`(async()=>{
      const refused=[];for(const address of ['https://example.com/one.md?lens=web','/one.html?lens=web','/one.md?lens=native']){
        refused.push(await wbDocsHost.load(address).then(()=>false,()=>true));}
      await __wbLoad('one');await wbDocsHost.reset();
      const stopped=await wbDocsHost.load('/two.md?lens=web').then(()=>false,()=>true);
      return {refused,stopped,ready:__workbenchReady,portals:document.querySelectorAll('[data-acme-portal]').length};
    })()`);
        assert.deepEqual(result, {
          refused: [true, true, true],
          stopped: true,
          ready: false,
          portals: 0,
        });
      },
    );

    await t.test(
      'reassignment applies the default scenario to fresh conditional Markdown',
      async () => {
        const target = await open();
        const result = await target.evaluate(`(async()=>{
        await __wbLoad('one');await wbDocsHost.load('/two.md?lens=web&scenario=default');
        await __wbWait(()=>__workbenchReady);
        return {scenario:document.documentElement.getAttribute('data-wb-scenario'),
          selected:!!document.getElementById('selected-only'),fallback:!!document.getElementById('default-only')};
      })()`);
        assert.deepEqual(result, { scenario: 'default', selected: false, fallback: true });
      },
    );

    await t.test('disposal cancels page timeouts, intervals and animation frames', async () => {
      const target = await open();
      const callbacks = await target.evaluate(`(async()=>{
        await __wbLoad('one');let callbacks=0;
        setTimeout(()=>callbacks++,30);setInterval(()=>callbacks++,30);
        requestAnimationFrame(()=>callbacks++);
        await wbDocsHost.reset();
        await new Promise(resolve=>__wbTestLater(resolve,80));
        return callbacks;
      })()`);
      assert.equal(callbacks, 0);
    });
  },
);
