/* Exercise the live DOM -> inert Electron mirror with two isolated renderers.
   Optional arguments: <project> <page>. Project files are never modified. */
var assert = require('node:assert/strict');
var fs = require('node:fs');
var path = require('node:path');
var os = require('node:os');
var runtime = require('../electron-runtime');
var engine = require('../electron-capture');
var server = require('../server');

async function main() {
  var work = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-mirror-'));
  console.log('Live mirror test: ' + work);
  var root = process.argv[2] ? path.resolve(process.argv[2]) : path.join(work, 'fixture');
  var page = process.argv[3] || 'index.html';
  var workbench = process.env.CANONIC_MIRROR_WORKBENCH === '1';
  if (!process.argv[2]) {
    fs.mkdirSync(root);
    fs.writeFileSync(path.join(root, 'index.html'), `<!doctype html><html><head><style>
      body { font:24px system-ui; background:#345; color:white; } x-card:not(:defined) { display:none }
      </style></head><body><h1 id="title">Live page</h1><x-card></x-card><script>
      window.appRuns = 1;
      customElements.define('x-card', class extends HTMLElement { constructor() { super();
        this.attachShadow({mode:'open'}).innerHTML = '<style>:host{display:block} input{font:24px system-ui} #scroll{height:100px;overflow:auto} #content{height:500px;background:linear-gradient(red,blue)} @keyframes slide{from{transform:translateX(0)}to{transform:translateX(100px)}} #animated{width:20px;height:20px;background:orange;animation:slide 2s linear infinite}</style><input value="original"><input type="checkbox"><div id="scroll"><div id="content">Scrollable</div></div><canvas width="80" height="80"></canvas><dialog>Modal</dialog><div popover>Popover</div><div id="animated"></div>';
        const sheet = new CSSStyleSheet(); sheet.replaceSync(':host{border:4px solid cyan}');
        this.shadowRoot.adoptedStyleSheets = [sheet];
        const canvas = this.shadowRoot.querySelector('canvas'); const ctx=canvas.getContext('2d'); ctx.fillStyle='orange';ctx.fillRect(0,0,80,80);
      }});
      </script></body></html>`);
  }
  if (workbench) {
    // Serve the selected files through a temporary project. Only generated handoffs and
    // the fixture manifest are writable outputs; neither links into the project.
    var selected = root;
    root = path.join(work, 'workbench'); fs.mkdirSync(root);
    fs.readdirSync(selected).filter(function (name) { return !['workbench.yaml', 'workbench.local.yaml', '.git', '.canonic'].includes(name); }).forEach(function (name) {
      fs.symlinkSync(path.join(selected, name), path.join(root, name));
    });
    fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Mirror test\nsections:\n  - name: Pages\n    items:\n      - label: Live page\n        src: ' + page + '\n');
  }
  var binary = await runtime.prepare(path.join(work, 'runtime'));
  var appDir = process.platform === 'darwin' ? path.resolve(binary, '../../Resources/app') : path.join(path.dirname(binary), 'resources/app');
  var mainSource = fs.readFileSync(path.join(__dirname, '../capture-helper/main.cjs'), 'utf8');
  mainSource += `\nif(process.env.CANONIC_MIRROR_TEST_SCALE) app.commandLine.appendSwitch('force-device-scale-factor', process.env.CANONIC_MIRROR_TEST_SCALE);`;
  mainSource += `\nvar originalHandle = handle; handle = function(request) {
    if (request.method === 'evaluate') return win.webContents.executeJavaScript(request.script);
    if (request.method === 'cdp') { benchmarkCDP=true; return {}; }
    return originalHandle(request);
  };
  var benchmarkCDP=false; var originalPNG=png;
  png=async function(timings,format) {
    if(!benchmarkCDP) return originalPNG(timings,format);
    var start=performance.now();
    if(!win.webContents.debugger.isAttached()) win.webContents.debugger.attach('1.3');
    var scale=await win.webContents.executeJavaScript('devicePixelRatio');
    var result=await win.webContents.debugger.sendCommand('Page.captureScreenshot', {
      format:format||'png',quality:90,fromSurface:true,captureBeyondViewport:false,optimizeForSpeed:true,
      clip:{x:0,y:0,width:width,height:height,scale:1/scale}
    });
    if(timings) timings.captureAndEncodeMs=performance.now()-start;
    return result.data;
  };`;
  fs.writeFileSync(path.join(appDir, 'main.cjs'), mainSource);
  var source = engine.create({ executable: binary });
  var target = engine.create({ executable: binary, spawn: function (file, args, options) {
    if (process.env.CANONIC_MIRROR_DPR) options.env.CANONIC_MIRROR_TEST_SCALE = process.env.CANONIC_MIRROR_DPR;
    return require('node:child_process').spawn(file, args, options);
  } });
  var captureProfile;
  var captureStarted;
  var captureOriginal = target.capture.bind(target);
  target.capture = function () { captureStarted = performance.now(); return captureOriginal.apply(null, arguments); };
  var sendOriginal = target.send.bind(target);
  target.send = async function (request) {
    if (request.method !== 'capture') return sendOriginal(request);
    var queueMs = performance.now() - captureStarted;
    var result = await sendOriginal(Object.assign({}, request, { profile: true }));
    captureProfile = Object.assign({ queueMs: queueMs }, result.timings);
    return result;
  };
  var running;
  var report = { root: root, page: page, samples: [] };
  var control = `Array.from(doc.querySelectorAll('*')).find(function(node){return node.shadowRoot && node.shadowRoot.querySelector('input');})`;
  function evaluate(helper, script) { return helper.send({ method: 'evaluate', script: script }); }
  try {
    running = await server.start({ root: root, capture: target, eagerCapture: workbench });
    var base = new URL(running.url).origin;
    if (process.env.CANONIC_MIRROR_CDP === '1') await target.send({ method: 'cdp' });
    if (workbench) {
      await source.preparePage({ url: base + '/_workbench/#' + page + '@1440', width: 1800, height: 1200, revision: 'workbench-test' });
      async function until(script) {
        var deadline = Date.now() + 20000;
        while (Date.now() < deadline) {
          var result = await evaluate(source, script); if (result) return result;
          await new Promise(function (resolve) { setTimeout(resolve, 100); });
        }
        throw new Error('Workbench test timed out: ' + script);
      }
      await until(`(function(){var doc=document.querySelector('iframe.is-active')?.contentDocument; return !!doc && !!(${control});})()`);
      await evaluate(source, `window.captureStart=0; window.captureResult=null;
        var fetchOriginal=window.fetch; window.fetch=function(url, options) {
          var frontendMs=performance.now()-captureStart;
          var result=fetchOriginal.apply(this,arguments);
          if(url==='/_workbench/capture') result.then(async function(res) {
            var response=await res.clone().json();
            if(!response.ok && /MIRROR_RESYNC/.test(response.error)) return;
            var mirror=JSON.parse(options.body).mirror;
            window.captureResult={elapsedMs:performance.now()-captureStart, frontendMs:frontendMs,
              requestBytes:options.body.length, patchNodes:mirror.nodes.length, patchStates:mirror.states.length,
              fullSnapshot:!mirror.base, result:response};
          }); return result;
        }; true`);
      for (var sample = 0; sample < Number(process.env.CANONIC_MIRROR_SAMPLES || 3); sample++) {
        // Let background synchronization run between captures. The third
        // changes a property without an event immediately before capture.
        await new Promise(function (resolve) { setTimeout(resolve, 1200); });
        await evaluate(source, `(function(){
          var doc=document.querySelector('iframe.is-active').contentDocument;
          if(${sample}>0) (${control}).shadowRoot.querySelector('input').value='Click proof ${sample}';
          window.captureResult=null; window.captureStart=performance.now();
          document.querySelector('[aria-label="Save screenshot"]').click(); return true;
        })()`);
        var result = await until('window.captureResult');
        result.profile = captureProfile;
        assert.equal(result.result.ok, true, JSON.stringify(result));
        if (sample > 0) assert.equal(await evaluate(target, `(function(){var doc=document.querySelector('iframe').contentDocument;return (${control}).shadowRoot.querySelector('input').value;})()`), 'Click proof ' + sample);
        report.samples.push(result); console.log(JSON.stringify(result));
        fs.writeFileSync(path.join(work, 'results.json'), JSON.stringify(report, null, 2));
      }
      return;
    }
    var payload = { url: new URL(page, base + '/').href, width: 1440, height: 1000,
      revision: 'live-test', format: 'jpeg', scroll: { x: 0, y: 0 }, markup: '' };
    await source.preparePage(payload);
    await evaluate(source, fs.readFileSync(path.join(__dirname, '../workbench/dom-mirror.js'), 'utf8') + '\nwindow.mirror = wbDOMMirror.create(document); true');
    await target.warm(base);
    if (!process.argv[2]) await evaluate(source, `var animation=document.querySelector('x-card').shadowRoot.getAnimations()[0]; animation.pause(); animation.currentTime=700; true`);
    for (var i = 0; i < 4; i++) {
      if (i === 1) await evaluate(source, `document.querySelector('#title, h1').textContent = 'Live mirror proof';
        var doc=document; var input = (${control}).shadowRoot.querySelector('input');
        input.value = 'Edited in the visible page';
        var card = document.querySelector('x-card');
        if(card) { var shadow = card.shadowRoot; shadow.querySelector('[type=checkbox]').checked = true;
          shadow.querySelector('#scroll').scrollTop=140; shadow.querySelector('dialog').showModal(); }
        true`);
      if (i === 2 && !process.argv[2]) await evaluate(source, `var shadow=document.querySelector('x-card').shadowRoot;
        shadow.querySelector('dialog').close(); shadow.querySelector('[popover]').showPopover();
        shadow.querySelector('#scroll').scrollTop=0; true`);
      if (i === 3 && !process.argv[2]) await evaluate(source, `var shadow=document.querySelector('x-card').shadowRoot;
        shadow.querySelector('input').value='Programmatic value without an event';
        shadow.adoptedStyleSheets[0].replaceSync(':host{border:4px solid lime}');
        shadow.querySelector('canvas').getContext('2d').clearRect(0,0,80,80); true`);
      var snapshot = await evaluate(source, '(function(){var start=performance.now();var data=window.lastSnapshot=mirror.read(true);return {data:data,serializeMs:performance.now()-start};})()');
      payload.mirror = snapshot.data;
      fs.writeFileSync(path.join(work, 'snapshot-' + i + '.json'), JSON.stringify(snapshot.data));
      var start = performance.now();
      await target.prepare(base, payload);
      var prepareMs = performance.now() - start;
      await evaluate(source, 'mirror.acknowledge(lastSnapshot); true');
      start = performance.now();
      var shot = await target.capture(base, payload);
      var captureMs = performance.now() - start;
      fs.writeFileSync(path.join(work, 'mirror-' + i + '.jpg'), shot);
      var original = await source.send({ method: 'capturePage', payload: payload, overlay: 'true', removeOverlay: 'true' });
      fs.writeFileSync(path.join(work, 'source-' + i + '.jpg'), Buffer.from(original.data, 'base64'));
      var mirrored = await evaluate(target, `(function(){var doc=document.querySelector('iframe').contentDocument;
        var card=(${control}); var shadow=card.shadowRoot;
        var retained=!window.previousCard || window.previousCard===card; window.previousCard=card;
        return {title:doc.querySelector('#title,h1').textContent, value:shadow.querySelector('input').value,
          retained:retained, border:doc.defaultView.getComputedStyle(card).borderTopColor,
          canvas:shadow.querySelector('canvas')?.getContext('2d').getImageData(0,0,1,1).data[3],
          animation:shadow.querySelector('#animated') && doc.defaultView.getComputedStyle(shadow.querySelector('#animated')).transform,
          scripts:doc.querySelectorAll('script').length, runs:doc.defaultView.appRuns || 0,
          scroll:shadow.querySelector('#scroll')?.scrollTop, checked:shadow.querySelector('[type=checkbox]')?.checked,
          modal:shadow.querySelector('dialog')?.matches(':modal'), popover:shadow.querySelector('[popover]')?.matches(':popover-open')}; })()`);
      assert.equal(mirrored.scripts, 0); assert.equal(mirrored.runs, 0);
      assert.equal(mirrored.retained, true);
      if (i > 0) { assert.equal(mirrored.title, 'Live mirror proof'); assert.equal(mirrored.value, i === 3 && !process.argv[2] ? 'Programmatic value without an event' : 'Edited in the visible page'); }
      if (!process.argv[2] && i === 1) { assert.equal(mirrored.checked, true); assert.equal(mirrored.scroll, 140); assert.equal(mirrored.modal, true); }
      if (!process.argv[2] && i === 2) { assert.equal(mirrored.scroll, 0); assert.equal(mirrored.modal, false); assert.equal(mirrored.popover, true); }
      if (!process.argv[2]) {
        assert.equal(mirrored.canvas, i === 3 ? 0 : 255); assert.equal(mirrored.border, i === 3 ? 'rgb(0, 255, 0)' : 'rgb(0, 255, 255)');
        assert.equal(mirrored.animation, 'matrix(1, 0, 0, 1, 35, 0)');
      }
      var sample = { serializeMs: snapshot.serializeMs, prepareMs: prepareMs, captureMs: captureMs, revision: snapshot.data.revision,
        animations: snapshot.data.animations.length, bytes: JSON.stringify(snapshot.data).length, mirrored: mirrored };
      report.samples.push(sample); console.log(JSON.stringify(sample));
      fs.writeFileSync(path.join(work, 'results.json'), JSON.stringify(report, null, 2));
    }
    if (!process.argv[2]) {
      async function flush() {
        payload.mirror = await evaluate(source, 'window.lastSnapshot=mirror.read(true)');
        await target.prepare(base, payload);
        await evaluate(source, 'mirror.acknowledge(lastSnapshot); true');
        return payload.mirror;
      }
      await evaluate(source, `var a=document.createElement('section');a.id='added';
        a.innerHTML='<span id="move">Before</span><b>Remove me</b>';document.body.append(a);
        var late=document.createElement('div');late.id='late';document.body.append(late);true`);
      await flush();
      await evaluate(target, `window.keptNode=document.querySelector('iframe').contentDocument.querySelector('#move'); true`);
      await evaluate(source, `document.querySelector('#move').firstChild.data='After';
        document.querySelector('#move').setAttribute('data-test','updated');
        document.querySelector('#late').append(document.querySelector('#move'));
        document.querySelector('#added').remove();
        document.querySelector('#late').attachShadow({mode:'open'}).innerHTML='<slot></slot><em>Late shadow</em>';
        true`);
      var structural = await flush();
      assert.ok(structural.base && structural.removed.length && structural.nodes.length);
      var actual = await evaluate(target, `(function(){var doc=document.querySelector('iframe').contentDocument;
        return {retained:doc.querySelector('#move')===keptNode,text:keptNode.textContent,
          parent:keptNode.parentElement.id,attr:keptNode.getAttribute('data-test'),removed:!doc.querySelector('#added'),
          shadow:doc.querySelector('#late').shadowRoot.textContent};})()`);
      assert.deepEqual(actual, { retained: true, text: 'After', parent: 'late', attr: 'updated', removed: true, shadow: 'Late shadow' });
      var unchanged = await flush();
      assert.equal(unchanged.nodes.length, 0); assert.equal(unchanged.states.length, 0);
      await evaluate(source, `document.querySelector('x-card').shadowRoot.querySelector('input').focus();true`);
      await flush();
      assert.equal(await evaluate(target, `document.querySelector('iframe').contentDocument.querySelector('x-card').shadowRoot.activeElement?.localName`), 'input');
      await evaluate(source, `window.detached=document.querySelector('#move');detached.remove();true`);
      await flush();
      await evaluate(source, `detached.setAttribute('data-test','while detached');detached.firstChild.data='Detached edit';document.querySelector('#late').append(detached);true`);
      await flush();
      assert.deepEqual(await evaluate(target, `(function(){var node=document.querySelector('iframe').contentDocument.querySelector('#move');return [node.textContent,node.getAttribute('data-test')];})()`), ['Detached edit', 'while detached']);
      await evaluate(source, `var outer=document.createElement('div');outer.id='outer';
        var inner=document.createElement('div');inner.id='inner';outer.append(inner);document.body.append(outer);true`);
      await flush();
      await evaluate(source, `var outer=document.querySelector('#outer'),inner=document.querySelector('#inner');
        document.body.append(inner);inner.append(outer);true`);
      await flush();
      assert.equal(await evaluate(target, `document.querySelector('iframe').contentDocument.querySelector('#outer').parentElement.id`), 'inner');
      // Simulate a skipped update followed by a helper missing the claimed base.
      await evaluate(source, `document.querySelector('x-card').shadowRoot.querySelector('input').value='Skipped';
        var skipped=mirror.read(true);mirror.acknowledge(skipped);
        document.querySelector('x-card').shadowRoot.querySelector('input').value='Requested';true`);
      payload.mirror = await evaluate(source, 'window.requested=mirror.read(true)');
      assert.equal(payload.mirror.nodes.length, 0); assert.equal(payload.mirror.states.length, 1);
      await assert.rejects(target.prepare(base, payload), /MIRROR_RESYNC/);
      await evaluate(source, `document.querySelector('x-card').shadowRoot.querySelector('input').value='Later';mirror.read(true);true`);
      payload.mirror = await evaluate(source, 'mirror.full(requested)');
      await target.prepare(base, payload);
      assert.equal(await evaluate(target, `document.querySelector('iframe').contentDocument.querySelector('x-card').shadowRoot.querySelector('input').value`), 'Requested');
      await evaluate(source, 'mirror.acknowledge(requested);true');
      await flush();
      assert.equal(await evaluate(target, `document.querySelector('iframe').contentDocument.querySelector('x-card').shadowRoot.querySelector('input').value`), 'Later');
      // Background preparation can complete while capture still holds an older
      // acknowledgement. Reversions and deletions must be included cumulatively.
      await evaluate(source, `var transient=document.createElement('p');transient.id='transient';document.body.append(transient);
        document.querySelector('x-card').shadowRoot.querySelector('input').value='Intermediate';
        document.querySelector('x-card').shadowRoot.querySelector('#scroll').scrollTop=140;true`);
      payload.mirror = await evaluate(source, 'mirror.read(true)');
      await target.prepare(base, payload);
      await evaluate(source, `document.querySelector('#transient').remove();
        document.querySelector('x-card').shadowRoot.querySelector('input').value='Later';
        document.querySelector('x-card').shadowRoot.querySelector('#scroll').scrollTop=0;true`);
      await flush();
      assert.deepEqual(await evaluate(target, `(function(){var doc=document.querySelector('iframe').contentDocument;
        var shadow=doc.querySelector('x-card').shadowRoot;return [!!doc.querySelector('#transient'),shadow.querySelector('input').value,shadow.querySelector('#scroll').scrollTop];})()`), [false, 'Later', 0]);
      report.protocol = { structuralEdits: true, stableIdentity: true, noOp: true, propertyOnlyPatch: true, exactRecovery: true, cumulativeReversions: true };
      fs.writeFileSync(path.join(work, 'results.json'), JSON.stringify(report, null, 2));
      console.log(JSON.stringify(report.protocol));
    }
  } finally {
    await source.close();
    if (running) await running.close(); else await target.close();
    await fs.promises.rm(path.join(work, 'runtime'), { recursive: true, force: true });
  }
}
main().catch(function (error) { console.error(error); process.exitCode = 1; });
