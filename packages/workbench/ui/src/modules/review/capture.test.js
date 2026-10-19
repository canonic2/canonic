import { test } from 'node:test';
import assert from 'node:assert/strict';
import { createRequire } from 'node:module';
import { createServer } from 'node:http';
import { fileURLToPath } from 'node:url';
import path from 'node:path';
import { createUiAssets } from '../../../../scripts/ui-browser-fixture.cjs';

const { Browser, findChrome } = createRequire(import.meta.url)('../../../../scripts/chrome.cjs');

void test(
  'standalone Docs capture omits canvas framing while placed artboards retain it',
  {
    skip: !findChrome() && 'Chrome is required for capture composition checks',
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
          res.end('<!doctype html>');
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
    const {capture}=await import('/_workbench/ui/src/modules/review/capture.js');
    const image=document.createElement('canvas');image.width=600;image.height=600;
    image.getContext('2d').fillStyle='red';image.getContext('2d').fillRect(0,0,600,600);
    const blob=await new Promise(resolve=>image.toBlob(resolve));
    const board={id:'docs',x:0,y:32,size:{width:600,height:600},
      target:{src:'docs.md',label:'Documentation',space:{id:'acme',name:'Acme'}},
      view:{src:'docs.md',label:'Documentation',ready:true,hash:'docs',payload:{docs:{}}}};
    const state={id:'canvas',revision:1,selected:'docs',artboards:[board]};
    const runtime=()=>({request:async()=>board.view,capture:async()=>({blob,payload:{src:board.target.src,docs:{},annotations:[]},at:1})});
    const captured=await capture(state,runtime);
    const bitmap=await createImageBitmap(captured.blob);image.getContext('2d').drawImage(bitmap,0,0);bitmap.close();
    const pixel=[...image.getContext('2d').getImageData(0,0,1,1).data];
    const placed=await capture({...state,artboards:[{...board,target:{...board.target,artboard:'placed'}}]},runtime);
    return {frame:captured.payload.frame,region:captured.payload.artboards[0].region,pixel,
      placedFrame:placed.payload.frame,placedRegion:placed.payload.artboards[0].region};
  })()`);
    assert.deepEqual(results.frame, { w: 600, h: 600 });
    assert.deepEqual(results.region, { x: 0, y: 0, width: 600, height: 600 });
    assert.ok(
      results.pixel[0] > 240 && results.pixel[1] < 10 && results.pixel[2] < 10,
      'the first captured pixel is the docs content, without a canvas margin or label',
    );
    assert.deepEqual(results.placedFrame, { w: 664, h: 696 });
    assert.deepEqual(results.placedRegion, { x: 32, y: 64, width: 600, height: 600 });
  },
);
