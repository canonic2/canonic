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
  'Configure pages preserves unexposed fields and keeps a rejected save editable',
  { skip: !findChrome() },
  async (t) => {
    const browser = new Browser();
    const assets = createUiAssets(
      path.resolve(fileURLToPath(new URL('../../../../', import.meta.url))),
    );
    const html = await readFile(new URL('../../app/workbench/index.html', import.meta.url), 'utf8');
    const start = html.indexOf('<dialog class="wb-config-dialog"');
    const dialog = html.slice(start, html.indexOf('</dialog>', start) + 9);
    const styles = [
      ...html.matchAll(/<link rel="stylesheet" href="(\/_workbench\/ui\/src\/[^"]+)"\s*\/>/g),
    ]
      .map((match) => match[0])
      .join('');
    const saves = [];
    const collections = [
      {
        name: 'Pages',
        items: [
          {
            label: 'Account',
            src: 'pages/account.html',
            scenarios: [{ id: 'error' }],
            code: 'src/account.js',
          },
        ],
      },
    ];
    const server = createServer((req, res) => {
      void (async () => {
        if (req.url === '/_workbench/config-file') {
          res.setHeader('Content-Type', 'application/json');
          if (req.method === 'POST') {
            let body = '';
            for await (const chunk of req) body += chunk;
            saves.push(JSON.parse(body));
            res.end(
              JSON.stringify(
                saves.length === 1 ? { ok: false, error: 'Acme write refused' } : { ok: true },
              ),
            );
          } else res.end(JSON.stringify({ ok: true, collections, sizes: ['fit', 'mobile'] }));
        } else if (req.url === '/') {
          res.setHeader('Content-Type', 'text/html');
          res.end(`<!doctype html>${styles}<button id="configureSpace">Configure pages</button>${dialog}
          <script src="/_workbench/ui/src/modules/project/configuration.js"></script>
          <script src="/_workbench/ui/src/views/configure-pages/config-editor.js"></script>
          <script src="/fixture.js"></script>`);
        } else if (req.url === '/fixture.js') {
          res.setHeader('Content-Type', 'text/javascript');
          res.end(`window.wbIcon=()=>document.createElementNS('http://www.w3.org/2000/svg','svg');
          const node=id=>document.getElementById(id);window.saved=0;
          wbConfigEditor.create({button:node('configureSpace'),dialog:node('configDialog'),body:node('configBody'),status:node('configStatus'),close:node('configClose'),addCollection:node('configAddCollection'),save:node('configSave'),read:wbProjectConfiguration.read,write:wbProjectConfiguration.write,onSaved:()=>saved++});window.fixtureReady=true;`);
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
    target.on('Runtime.exceptionThrown', (event) => errors.push(event.exceptionDetails.text));
    async function until(expression) {
      const deadline = Date.now() + 5000;
      while (!(await target.evaluate(expression))) {
        if (Date.now() > deadline)
          throw Error('Configure pages did not settle: ' + expression + '\n' + errors.join('\n'));
        await new Promise((resolve) => setTimeout(resolve, 20));
      }
    }
    await target.navigate('http://127.0.0.1:' + server.address().port);
    await until('window.fixtureReady');
    await target.evaluate(`document.getElementById('configureSpace').click()`);
    await until(`!!document.querySelector('.wb-config-nav-page')`);
    assert.equal(
      await target.evaluate(
        `getComputedStyle(document.querySelector('.wb-config-workspace')).display`,
      ),
      'grid',
    );
    await target.evaluate(
      `document.querySelector('.wb-config-nav-page').click();const input=document.querySelector('.wb-config-field input');input.value='Edited account';input.dispatchEvent(new Event('input'));document.getElementById('configSave').click()`,
    );
    await until(
      `document.getElementById('configStatus').textContent==='Acme write refused' && !document.getElementById('configSave').disabled`,
    );
    assert.equal(
      await target.evaluate(
        `document.getElementById('configDialog').open && document.querySelector('.wb-config-field input').value==='Edited account'`,
      ),
      true,
    );
    await target.evaluate(`document.getElementById('configSave').click()`);
    await until(`window.saved===1 && !document.getElementById('configDialog').open`);
    assert.equal(saves.length, 2);
    assert.deepEqual(saves[1].collections[0].items[0], {
      ...collections[0].items[0],
      label: 'Edited account',
    });
    assert.deepEqual(errors, []);
  },
);
