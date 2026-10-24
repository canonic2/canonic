import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { createUiAssets } from '../../../../scripts/ui-browser-fixture.cjs';

const { Browser, findChrome } = createRequire(import.meta.url)('../../../../scripts/chrome.cjs');

void test(
  'canvas reading mode owns presentation while the host applies viewport changes',
  {
    skip: !findChrome() && 'Chrome is required for Web Component interaction checks',
  },
  async (t) => {
    const browser = new Browser();
    t.after(() => browser.close());
    const serve = createUiAssets(
      path.resolve(fileURLToPath(new URL('../../../..', import.meta.url))),
    );
    const server = createServer((req, res) => {
      void (async () => {
        if (req.url.startsWith('/_workbench/')) {
          const asset = await serve(req.url);
          res.writeHead(asset.status, { 'Content-Type': asset.type });
          res.end(asset.body);
        } else {
          res.setHeader('Content-Type', 'text/html');
          res.end(`<!doctype html><wb-canvas id="canvas" style="width:800px;height:600px;--wb-accent:blue"></wb-canvas>
          <script>document.getElementById('canvas').reading=true;</script>
          <script type="module">
          import {WbCanvas} from '/_workbench/ui/src/components/canvas/element.js';
          customElements.define('wb-canvas',WbCanvas);
          </script>`);
        }
      })().catch((error) => {
        res.statusCode = 500;
        res.end(String(error));
      });
    });
    await new Promise((resolve) => server.listen(0, '127.0.0.1', resolve));
    t.after(() => new Promise((resolve) => server.close(resolve)));
    await browser.launch();
    const target = await browser.createTarget();
    await target.navigate(`http://127.0.0.1:${server.address().port}/`);
    const results = await target.evaluate(`(async()=>{
    await Promise.race([customElements.whenDefined('wb-canvas'),new Promise((_,reject)=>setTimeout(()=>reject(new Error('Canvas registration failed')),3000))]);
    const canvas=document.getElementById('canvas'), results=[];
    const check=(value,label)=>{if(!value)throw new Error(label);results.push(label)};
    const tick=()=>new Promise(r=>requestAnimationFrame(()=>requestAnimationFrame(r)));
    const scroll=[], sizes=[], selected=[];
    const preparing=document.createElement('wb-canvas');
    preparing.style.cssText='width:800px;height:600px';document.body.append(preparing);
    const pending=document.createElement('iframe');pending.slot='pending';
    const loaded=new Promise(resolve=>pending.onload=resolve);
    pending.srcdoc='<input value="kept">';preparing.append(pending);
    const destination={id:'pending',x:0,y:32,size:{width:320,height:180},target:{src:'next.html',space:{id:'acme'}},view:{ready:false}};
    preparing.staged=[destination];
    await loaded;await tick();
    check(pending.clientWidth===320 && pending.clientHeight===180,'pending renderer receives its logical viewport before readiness');
    check(preparing.state.artboards.length===0 && !preparing.shadowRoot.querySelector('section'),'preparation does not publish a displayed artboard');
    const pendingDocument=pending.contentDocument;
    preparing.state={id:'preparing',revision:1,selected:'pending',artboards:[{...destination,view:{ready:true}}]};
    preparing.staged=[];await tick();
    check(pending.contentDocument===pendingDocument && pending.contentDocument.querySelector('input').value==='kept','ready promotion preserves the prepared document');
    preparing.remove();
    canvas.addEventListener('wb-reading-scroll',e=>scroll.push(e.detail));
    canvas.addEventListener('wb-reading-resize',e=>{
      sizes.push(e.detail);
      canvas.state={...canvas.state,artboards:canvas.state.artboards.map(b=>({...b,size:{width:e.detail.width,height:e.detail.height}}))};
    });
    canvas.addEventListener('wb-artboard-select',e=>selected.push(e.detail.id));
    canvas.state={id:'fixture',revision:1,selected:'one',artboards:[{
      id:'one',x:0,y:32,size:{width:800,height:600},
      target:{src:'docs.md',label:'Documentation',space:{id:'acme',name:'Acme'}},view:{label:'Documentation',ready:true}
    }]};
    await tick();
    const section=canvas.shadowRoot.querySelector('section'), label=section.querySelector('button');
    check(canvas.reading && canvas.hasAttribute('reading'),'pre-upgrade reading input is preserved and reflected');
    check(label.getClientRects().length===0 && getComputedStyle(section).boxShadow==='none','reading surface has no label or selection shadow');
    check(canvas.scale===1 && section.getBoundingClientRect().width===800,'reading starts at 100% without fitting margins');
    canvas.zoomTo(.5);
    check(canvas.state.artboards[0].size.height===1200 && canvas.state.artboards[0].size.width===800,'zoom changes visible page height without changing layout width');
    check(scroll.at(-1).from===1 && scroll.at(-1).to===.5 && scroll.at(-1).pointY===300,'host receives the zoom anchor for page scrolling');
    check(Math.round(section.getBoundingClientRect().x-canvas.getBoundingClientRect().x)===200,'zoomed-out documentation stays horizontally centered');
    canvas.dispatchEvent(new WheelEvent('wheel',{deltaY:40,cancelable:true}));
    check(scroll.at(-1).dy===80,'wheel on the reading canvas scrolls in page pixels');
    canvas.style.width='700px'; await tick();
    check(canvas.scale===.5 && canvas.state.artboards[0].size.width===700,'editor resizing preserves zoom and updates the layout width');
    canvas.fitAll();
    check(canvas.scale===1 && canvas.state.artboards[0].size.height===600,'fit restores the reading viewport at 100%');
    canvas.reading=false;
    check(label.getClientRects().length>0 && getComputedStyle(section).boxShadow!=='none','preview mode restores normal artboard labels and selection');
    label.click();check(selected.length===1 && selected[0]==='one','normal artboard selection still emits user intent');
    canvas.dispatchEvent(new WheelEvent('wheel',{deltaY:-10,ctrlKey:true,cancelable:true}));
    const previewScale=canvas.scale;canvas.state={...canvas.state};
    check(canvas.scale===previewScale,'preview wheel zoom survives subsequent state updates');
    canvas.reading=true;canvas.remove();const count=sizes.length;
    canvas.style.width='600px';await tick();
    check(sizes.length===count,'disconnect releases the viewport observer');
    document.body.append(canvas);await tick();
    check(canvas.state.artboards[0].size.width===600,'reconnect reacquires the viewport observer');
    return results;
  })()`);
    assert.equal(results.length, 17);
  },
);
