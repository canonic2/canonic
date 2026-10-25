/* Production resize acceptance. The same assertions can run in an editor canvas
   through verifyResize(evaluate, resizeHost); no project compiler is needed. */
const assert = require('node:assert/strict');
const fs = require('node:fs');
const os = require('node:os');
const path = require('node:path');
const { fit } = require('../ui/src/modules/canvas/geometry.js');
const server = require('../server/node/server');
const { Browser } = require('./chrome.cjs');

function createFixture(root) {
  fs.writeFileSync(path.join(root, 'workbench.yaml'), `name: Acme
previews: false
sizes:
  fit: true
  sidebar:
    width: 340
    height: fill
    button: true
  banner:
    width: fill
    height: 280
    button: true
  wide:
    width: 1600
    height: fill
    button: true
  fixed:
    width: 800
    height: 600
    button: true
  resizable: true
collections:
  - name: Pages
    items:
      - label: Form
        src: form.html
`);
  fs.writeFileSync(path.join(root, 'form.html'), `<!doctype html><style>
    body{margin:0}#viewport{width:100vw;height:100vh}footer{height:2000px}
    </style><input id="edit" value="Original"><div id="viewport"></div><footer></footer>`);
}

async function verifyResize(evaluate, resizeHost) {
  async function until(expression) {
    const deadline = Date.now() + 10000;
    while (!await evaluate(expression)) {
      if (Date.now() > deadline) throw Error('Resize did not settle: ' + expression + '\n' + await evaluate('JSON.stringify(window.__resizeProbe?.read())'));
      await new Promise(resolve => setTimeout(resolve, 20));
    }
  }
  await until(`document.getElementById('artboards')?.state.artboards[0]?.view?.ready && !document.getElementById('artboards').inert`);
  await evaluate(`window.__resizeProbe={
    canvas:document.getElementById('artboards'),
    renderer:document.querySelector('wb-canvas > iframe'),
    read(){
      const canvas=this.canvas,board=canvas.state.artboards[0];
      const renderer=this.renderer,frame=renderer.contentDocument.querySelector('iframe.is-active');
      const doc=frame.contentDocument,box=doc.getElementById('viewport').getBoundingClientRect();
      const world=new DOMMatrix(getComputedStyle(canvas.shadowRoot.querySelector('.world')).transform);
      return {size:board.size,room:{width:canvas.clientWidth,height:canvas.clientHeight},
        boards:canvas.state.artboards.map(b=>({x:b.x,y:b.y,size:b.size})),
        camera:{x:world.e,y:world.f,scale:canvas.scale},
        viewport:{width:frame.contentWindow.innerWidth,height:frame.contentWindow.innerHeight},
        units:{width:box.width,height:box.height},
        retained:renderer===document.querySelector('wb-canvas > iframe') && frame===this.frame && doc===this.doc,
        value:doc.getElementById('edit').value,scroll:frame.contentWindow.scrollY};
    }
  };
  __resizeProbe.frame=__resizeProbe.renderer.contentDocument.querySelector('iframe.is-active');
  __resizeProbe.doc=__resizeProbe.frame.contentDocument;
  __resizeProbe.doc.getElementById('edit').value='Retained edit';
  __resizeProbe.frame.contentWindow.scrollTo(0,200);`);
  const presets = [
    ['fit', null, null], ['sidebar', 340, null], ['banner', null, 280],
    ['wide', 1600, null], ['fixed', 800, 600], ['resizable', 'keep', 'keep'],
  ];
  const results = [];
  async function resize(width, height) {
    const before = (await evaluate('__resizeProbe.read()')).room;
    await resizeHost(width, height);
    await until(`(()=>{const room=__resizeProbe.read().room;return room.width!==${before.width} || room.height!==${before.height}})()`);
  }
  for (const [key, width, height] of presets) {
    await resizeHost(1280, 900);
    await evaluate(`document.getElementById('sizeSwitcher').shadowRoot.querySelector('[data-key="${key}"]').click()`);
    await until(`document.getElementById('sizeSwitcher').current==='${key}' && !document.getElementById('artboards').inert`);
    // Size picks may arrive before the renderer's size response.
    await until(`(()=>{const s=__resizeProbe.read();return s.size.width===s.viewport.width && s.size.height===s.viewport.height})()`);
    const kept = (await evaluate('__resizeProbe.read()')).size;
    async function settled() {
      await until(`(()=>{const s=__resizeProbe.read();return s.size.width===${width === 'keep' ? kept.width : width ?? 'Math.max(320,s.room.width)'} && s.size.height===${height === 'keep' ? kept.height : height ?? 'Math.max(320,s.room.height)'} && s.size.width===s.viewport.width && s.size.height===s.viewport.height})()`);
      // Drain the resize observers and their resulting presentation writes.
      await evaluate('new Promise(resolve=>requestAnimationFrame(()=>requestAnimationFrame(resolve)))');
      const state = await evaluate('__resizeProbe.read()');
      assert.deepEqual(state.units, state.size, key + ': viewport units follow logical artboard dimensions');
      assert.equal(state.retained, true, key + ': resizing keeps renderer, iframe and document');
      assert.equal(state.value, 'Retained edit');
      assert.equal(state.scroll, 200, key + ': resizing preserves page scroll');
      return state;
    }
    await settled();
    await evaluate('__resizeProbe.canvas.fitAll()');
    for (const [w, h] of [[1120, 760], [1460, 1000]]) {
      await resize(w, h);
      const state = await settled();
      const expected = fit(state.boards, state.room);
      for (const field of ['x', 'y', 'scale']) assert.ok(Math.abs(state.camera[field] - expected[field]) < 0.01, key + ': automatic fit follows the host size: ' + field);
    }
    await evaluate(`__resizeProbe.canvas.zoomTo(0.5);__resizeProbe.canvas.dispatchEvent(new WheelEvent('wheel',{deltaX:37,deltaY:61,cancelable:true}))`);
    const manual = (await evaluate('__resizeProbe.read()')).camera;
    for (const [w, h] of [[1180, 800], [1380, 940]]) {
      await resize(w, h);
      assert.deepEqual((await settled()).camera, manual, key + ': host resize preserves manual pan and zoom');
    }
    results.push({ key, size: (await evaluate('__resizeProbe.read()')).size, retained: true, camera: true });
  }
  return results;
}

async function main() {
  const root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-resize-'));
  const browser = new Browser();
  let running;
  try {
    createFixture(root);
    running = await server.start({ root, capture: { close() {} } });
    await browser.launch();
    const target = await browser.createTarget();
    const errors = [];
    target.on('Runtime.exceptionThrown', event => errors.push(event.exceptionDetails.exception?.description || event.exceptionDetails.text));
    const resize = (width, height) => target.send('Emulation.setDeviceMetricsOverride', { width, height, deviceScaleFactor: 1, mobile: false });
    await resize(1280, 900);
    await target.navigate(running.url);
    const results = await verifyResize(expression => target.evaluate(expression), resize);
    assert.deepEqual(errors, []);
    console.log(JSON.stringify({ host: 'Chrome', results }));
  } finally {
    await browser.close();
    await running?.close();
    fs.rmSync(root, { recursive: true, force: true });
  }
}

module.exports = { createFixture, verifyResize };
if (require.main === module) main().catch(error => { console.error(error); process.exitCode = 1; });
