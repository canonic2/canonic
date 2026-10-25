import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { createUiAssets } from '../../../../scripts/ui-browser-fixture.cjs';

const require = createRequire(import.meta.url);
const { Browser, findChrome } = require('../../../../scripts/chrome.cjs');

test(
  'the authored sidebar loads its project adapter and page-list view under CSP',
  {
    skip: !findChrome() && 'Chrome is required',
  },
  async (t) => {
    const browser = new Browser();
    const assets = createUiAssets(
      path.resolve(fileURLToPath(new URL('../../../../', import.meta.url))),
    );
    const config = {
      name: 'Acme',
      previews: true,
      implementations: {},
      problems: [],
      collections: [
        {
          name: 'Pages',
          icon: 'file-text',
          items: [
            {
              group: 'Auth',
              items: [
                {
                  src: 'sign-in.html',
                  label: 'Sign in',
                  scenarios: [
                    { id: 'default', label: 'Default' },
                    { id: 'error', label: 'Wrong password' },
                  ],
                },
              ],
            },
            { src: 'home.html', label: 'Home' },
          ],
        },
        { name: 'Components', icon: 'component', items: [{ src: 'button.html', label: 'Button' }] },
      ],
    };
    const server = createServer((req, res) => {
      void (async () => {
        const url = new URL(req.url, 'http://example.com');
        if (url.pathname === '/') {
          const html = await readFile(new URL('./sidebar.html', import.meta.url), 'utf8');
          res.setHeader('Content-Type', 'text/html');
          res.end(
            html
              .replace('<script type="importmap">', '<script type="importmap" nonce="fixture">')
              .replace(
                '<head>',
                `<head><base href="/_workbench/"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'self' 'nonce-fixture'; style-src 'self'; img-src 'self' data:; connect-src 'self'">`,
              ),
          );
        } else if (url.pathname === '/_workbench/config.js') {
          res.setHeader('Content-Type', 'text/javascript');
          res.end(`window.errors=[];window.violations=[];window.posted=[];
          addEventListener('error',e=>errors.push(e.message));
          document.addEventListener('securitypolicyviolation',e=>violations.push(e.violatedDirective));
          window.acquireVsCodeApi=()=>({postMessage(message){posted.push(message);if(message.type==='canonic-catalog-request')window.postMessage({type:'canonic-catalog',collections:[],problems:['Acme diagnostic']},'*')}});
          window.wbConfig={load(ok){ok(${JSON.stringify(config)})}};`);
        } else if (['/_workbench/manifest.js', '/_workbench/yaml.js'].includes(url.pathname)) {
          res.setHeader('Content-Type', 'text/javascript');
          res.end(
            await readFile(
              new URL('../../../../contracts/' + url.pathname.split('/').at(-1), import.meta.url),
            ),
          );
        } else if (url.pathname === '/node_modules/lucide/dist/umd/lucide.min.js') {
          res.setHeader('Content-Type', 'text/javascript');
          res.end(await readFile(require.resolve('lucide/dist/umd/lucide.min.js')));
        } else {
          const asset = await assets(
            url.pathname === '/_workbench/icons.js'
              ? '/_workbench/ui/legacy/icons.js'
              : url.pathname,
          );
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
    await target.navigate('http://127.0.0.1:' + server.address().port);
    await target.evaluate(`(async()=>{
    const deadline=performance.now()+5000;
    while(!posted.some(m=>m.type==='canonic-ready')){
      if(performance.now()>deadline)throw Error('Sidebar did not start: '+JSON.stringify({errors,violations}));
      await new Promise(resolve=>setTimeout(resolve,10));
    }
    const search=document.getElementById('search');
    if(search.disabled)throw Error('Search stays disabled after catalog delivery');
    document.querySelector('#pageList button[data-src="sign-in.html"]').click();
    document.querySelector('#pageList [data-scenario="error"]').click();
    if(!document.querySelector('#pageList [data-scenario="error"]').matches('[aria-current="true"]'))throw Error('Pick is not marked');
    search.value='Button';search.dispatchEvent(new Event('input'));
    if(document.querySelectorAll('#pageList .wb-item').length!==1)throw Error('Search did not cross collections');
    window.postMessage({type:'canonic-catalog',collections:[],problems:['Updated diagnostic']},'*');
    await new Promise(resolve=>setTimeout(resolve,0));
    if(search.value!=='Button'||document.getElementById('diagnostics').textContent!=='Updated diagnostic')throw Error('Diagnostics replaced the list state');
    search.value='';search.dispatchEvent(new Event('input'));
    document.querySelector('#collections [data-collection="Components"]').click();
  })()`);
    const lattice = await target.evaluate(
      `performance.getEntriesByType('resource').map(entry=>new URL(entry.name).pathname).filter(path=>path.includes('/node_modules/@canonic2/lattice/'))`,
    );
    assert.ok(lattice.some((path) => path.endsWith('/src/index.js')));
    assert.ok(lattice.some((path) => path.endsWith('/src/modules/elements/elements.js')));
    const result =
      await target.evaluate(`({errors,violations,picks:posted.filter(m=>m.type==='canonic-pick').map(m=>m.hash),
    sheet:[...document.styleSheets].some(s=>s.href?.endsWith('/views/page-list/page-list.css')&&s.cssRules.length>0),
    title:document.getElementById('collectionName').textContent})`);
    assert.deepEqual(result, {
      errors: [],
      violations: [],
      picks: ['sign-in.html', 'sign-in.html:error', 'button.html'],
      sheet: true,
      title: 'Components',
    });
  },
);
