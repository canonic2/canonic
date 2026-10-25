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
  'legacy annotation controls keep tool toggles, shape menus, counts and review actions',
  { skip: !findChrome() },
  async (t) => {
    const browser = new Browser();
    const assets = createUiAssets(
      path.resolve(fileURLToPath(new URL('../../../../', import.meta.url))),
    );
    const html = await readFile(new URL('../../app/workbench/index.html', import.meta.url), 'utf8');
    const start = html.indexOf('<div class="wb-toolbar"');
    const toolbar = html.slice(start, html.indexOf('<div class="wb-view-controls"', start));
    const server = createServer((req, res) => {
      void (async () => {
        if (req.url === '/') {
          res.setHeader('Content-Type', 'text/html');
          res.end(`<!doctype html><link rel="stylesheet" href="/_workbench/ui/src/theme/defaults.css"><link rel="stylesheet" href="/_workbench/ui/src/views/toolbar/toolbar.css">${toolbar}
          <script src="/_workbench/ui/src/views/toolbar/legacy-presentation.js"></script><script src="/fixture.js"></script>`);
        } else if (req.url === '/fixture.js') {
          res.setHeader('Content-Type', 'text/javascript');
          res.end(`window.wbIcon=()=>document.createElementNS('http://www.w3.org/2000/svg','svg');
          window.tool='pointer';window.shape='rect';window.calls=[];
          window.toolbar=wbAnnotationToolbar();
          toolbar.connect({tool:()=>tool,shape:()=>shape,setTool:name=>{window.tool=name;toolbar.tool(name)},setShape:name=>{window.shape=name;toolbar.shape(name);window.tool=name;toolbar.tool(name)},undo:()=>calls.push('undo'),clear:()=>calls.push('clear'),shoot:()=>calls.push('shoot'),handoff:()=>calls.push('handoff')});
          toolbar.shape(shape);toolbar.tool(tool);window.fixtureReady=true;`);
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
    await target.navigate('http://127.0.0.1:' + server.address().port);
    const deadline = Date.now() + 5000;
    while (!(await target.evaluate('window.fixtureReady'))) {
      if (Date.now() > deadline) throw Error('Toolbar did not initialize: ' + errors.join('\n'));
      await new Promise((resolve) => setTimeout(resolve, 20));
    }
    const result = await target.evaluate(`(()=>{
    const draw=document.querySelector('[data-tool=draw]');draw.click();const drawing=tool==='draw'&&draw.getAttribute('aria-pressed')==='true';draw.click();const toggled=tool==='pointer';
    document.getElementById('shapeCaret').click();const expanded=document.getElementById('shapeCaret').getAttribute('aria-expanded');document.querySelector('[data-shape=ellipse]').click();
    const shapeState=[tool,document.getElementById('shapeTool').getAttribute('aria-label'),document.querySelector('[data-shape=ellipse]').getAttribute('aria-current')];
    document.getElementById('shapeCaret').click();document.body.dispatchEvent(new PointerEvent('pointerdown',{bubbles:true}));
    toolbar.count(2);const count=document.getElementById('annotationCount').textContent;toolbar.count(0);
    toolbar.enableHandoff();toolbar.busyHandoff(true);const busy=document.getElementById('handoff').disabled&&document.getElementById('handoff').getAttribute('aria-busy')==='true';toolbar.busyHandoff(false);
    for(const id of ['undo','clearAnnotations','handoff'])document.getElementById(id).click();
    return {drawing,toggled,expanded,shapeState,closed:document.getElementById('shapeMenu').hidden,count,empty:document.getElementById('annotationCount').hidden,busy,released:!document.getElementById('handoff').disabled&&!document.getElementById('handoff').hasAttribute('aria-busy'),calls};
  })()`);
    assert.equal(
      result.drawing &&
        result.toggled &&
        result.closed &&
        result.empty &&
        result.busy &&
        result.released,
      true,
    );
    assert.equal(result.expanded, 'true');
    assert.deepEqual(result.shapeState, ['ellipse', 'Circle', 'true']);
    assert.equal(result.count, '2 annotations');
    assert.deepEqual(result.calls, ['undo', 'clear', 'handoff']);
    assert.deepEqual(errors, []);
  },
);
