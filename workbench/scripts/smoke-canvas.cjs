/* Browser acceptance for multi-artboard support through a temporary harness.
   No production controls or canvas layout are installed by this check. */
var assert = require('node:assert/strict');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var server = require('../server');
var spaces = require('../spaces');
var Browser = require('./chrome.cjs').Browser;
var wait = function (ms) { return new Promise(function (resolve) { setTimeout(resolve, ms); }); };

async function main() {
  var folder = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-canvas-support-'));
  var roots = ['product','system'].map(function (name) {
    var root = path.join(folder, name); fs.mkdirSync(root);
    fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: ' + name + '\ncollections:\n  - name: Pages\n    items:\n      - label: Form\n        src: form.html\n        states:\n          - id: default\n            label: Default\n          - id: error\n            label: Error\n');
    fs.writeFileSync(path.join(root, 'form.html'), '<!doctype html><html><body><h1>' + name + '</h1><input aria-label="Name"><button>Submit</button></body></html>');
    // Test-only host: the shipping index.html and its controls stay intact.
    fs.writeFileSync(path.join(root, 'harness.html'), '<!doctype html><html><body></body></html>');
    return root;
  });
  var handoff, browser = new Browser();
  var hub = spaces.create({ start: function (space) { return server.start({ root: space.root, spaces: hub, onHandoff: function (text, payload) { handoff = { text: text, payload: payload }; } }); } });
  hub.set(roots.map(function (dir) { return { dir: dir }; }));
  try {
    var listed = hub.list(), running = await hub.open(listed[0].id), other = await hub.open(listed[1].id);
    var registry = listed.map(function (p, i) { return { id: p.id, name: p.name, root: p.root, url: i ? other.url : running.url }; });
    await browser.launch(); var t = await browser.createTarget(); await t.enable();
    var errors = []; t.on('Runtime.exceptionThrown', function (event) { errors.push(event.exceptionDetails.exception && event.exceptionDetails.exception.description || event.exceptionDetails.text); });
    await t.navigate(new URL('/harness.html', running.url).href);
    await t.evaluate('(async()=>{' +
      'const {createController}=await import("/_workbench/src/canvas/controller.ts");' +
      'const {createRuntime}=await import("/_workbench/src/canvas/runtime.ts");' +
      'const {capture}=await import("/_workbench/src/canvas/review.ts");' +
      'const {createPublisher}=await import("/_workbench/src/canvas/publication.ts");' +
      'window.registry=' + JSON.stringify(registry) + ';window.supportErrors=[];' +
      'window.controller=createController({' +
        'mount(board,callbacks){const r=createRuntime(board,callbacks);r.element.dataset.board=board.id;document.body.append(r.element);return r;},' +
        'render(state){for(const board of state.artboards){const f=document.querySelector("iframe[data-board=\\""+board.id+"\\"]");if(f){f.width=board.size.width;f.height=board.size.height;}}},' +
        'host(){},error(e){supportErrors.push(String(e));}});' +
      'window.review=capture;' +
      'window.publisher=createPublisher({read:()=>({state:controller.snapshot(),browsed:registry[0].id,spaces:registry,activity:Date.now()}),' +
        'async send(request){const r=await fetch("/_workbench/canvas/space",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(request)});if(!r.ok)throw Error(await r.text());},' +
        'withdraw(request){fetch("/_workbench/canvas/space",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify(request)});},error(e){supportErrors.push(String(e));}});' +
      'window.first=controller.create({space:registry[0],src:"form.html",state:null,lens:null},{width:1512,height:982});' +
      'window.second=controller.create({space:registry[0],src:"form.html",state:null,lens:null},{width:393,height:852});' +
    '})()');
    await wait(1800);
    await t.evaluate('controller.command(second,"state","1")'); await wait(700);
    var boards = await t.evaluate('controller.snapshot().artboards.map(b=>({size:b.size,state:b.view.state,innerWidth:controller.runtime(b.id).element.contentDocument.querySelector("#frame").contentWindow.innerWidth}))');
    assert.equal(boards.length, 2); assert.equal(boards[0].innerWidth, 1512); assert.equal(boards[1].innerWidth, 393);
    assert.equal(boards[0].state, null); assert.equal(boards[1].state, 'error');
    await t.evaluate('controller.runtime(first).element.contentDocument.querySelector("#frame").contentDocument.querySelector("input").value="First only"');
    assert.equal(await t.evaluate('controller.runtime(second).element.contentDocument.querySelector("#frame").contentDocument.querySelector("input").value'), '');
    await t.evaluate('window.third=controller.create({space:registry[1],src:"form.html",state:null,lens:null},{width:1512,height:982})');
    await wait(1800); await t.evaluate('publisher.publish()');
    for (var instance of [running, other]) {
      var context = (await (await fetch(new URL('/_workbench/view', instance.url))).json()).view;
      assert.equal(context.artboards.length, 3);
      assert.equal(new Set(context.artboards.map(function (b) { return b.space.id; })).size, 2);
    }
    await t.evaluate('window.captured=null;window.captureError=null;controller.capture(review).then(v=>{window.captured=v;},e=>{window.captureError=String(e);})');
    for (var i = 0; i < 45; i++) {
      await wait(1000); if (await t.evaluate('!!captured || !!captureError')) break;
    }
    assert.equal(await t.evaluate('captureError'), null);
    assert.ok(await t.evaluate('!!captured'), 'complete capture acquired');
    await t.evaluate('(async()=>{const shot=await fetch("/_workbench/shot?name=workbench-canvas",{method:"POST",headers:{"Content-Type":"image/jpeg"},body:captured.blob});const saved=await shot.json();if(!shot.ok)throw Error(saved.error);const result=await fetch("/_workbench/handoff",{method:"POST",headers:{"Content-Type":"application/json"},body:JSON.stringify({...captured.payload,file:saved.file})});if(!result.ok)throw Error(await result.text());})()');
    assert.ok(handoff); assert.equal(handoff.payload.artboards.length, 3);
    assert.match(handoff.text, /product/); assert.match(handoff.text, /system/);
    assert.equal(handoff.payload.frame.w, 3577); assert.equal(handoff.payload.frame.h, 1078);
    assert.ok(fs.existsSync(path.join(roots[0], handoff.payload.file)));
    await t.evaluate('controller.close(second)');
    assert.equal(await t.evaluate('controller.snapshot().artboards.length'), 2);
    await t.evaluate('publisher.dispose();controller.dispose()');
    assert.equal(await t.evaluate('document.querySelectorAll("iframe[data-board]").length'), 0);
    assert.deepEqual(await t.evaluate('supportErrors'), []);
    assert.deepEqual(errors, []);
    console.log('Canvas support passed: isolated instances, cross-space context, full-canvas capture/handoff, and disposal through injected ports.');
  } finally { await browser.close(); await hub.close(); fs.rmSync(folder, { recursive: true, force: true }); }
}
main().catch(function (error) { console.error(error); process.exitCode = 1; });
