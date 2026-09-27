/* Opt-in native integration check. Uses the packaged runtime and a temporary
   fixture; npm test itself requires no browser or desktop session. */
var assert = require('node:assert/strict');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var http = require('node:http');
var engine = require('../electron-capture');
var server = require('../server');
async function until(check) {
  var deadline = Date.now() + 15000;
  while (!check()) {
    if (Date.now() > deadline) throw new Error('Helper did not recover');
    await new Promise(function (resolve) { setTimeout(resolve, 25); });
  }
}
async function main() {
  assert.ok(engine.available(), 'Run npm run bundle-capture on a desktop first');
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-capture-smoke-'));
  var helper = engine.create({ inject: fs.readFileSync(path.join(__dirname, '../workbench/describe.js'), 'utf8') +
    '\nwindow.__wbDescribeAt = function(x, y) { return window.wbDescribe.at(document, x, y); };' });
  var loads = 0;
  var lazyLoads = 0;
  var external = http.createServer(function (req, res) {
    if (req.url === '/lazy.svg') {
      lazyLoads++;
      res.setHeader('Content-Type', 'image/svg+xml');
      res.end('<svg xmlns="http://www.w3.org/2000/svg" width="100" height="100"><rect width="100" height="100" fill="red"/></svg>');
      return;
    }
    loads++; res.setHeader('Content-Type', 'text/html');
    res.end('<html style="scroll-behavior:smooth"><body style="background:#def;font:32px sans-serif"><h1>Implementation</h1>' +
      '<img loading="lazy" src="/lazy.svg" width="100" height="100" style="position:absolute;top:20000px;left:0">' +
      '<div style="height:21000px"></div></body></html>');
  });
  await new Promise(function (resolve) { external.listen(0, '127.0.0.1', resolve); });
  var externalUrl = 'http://127.0.0.1:' + external.address().port + '/';
  fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Capture test\nimplementations:\n  app:\n    kind: url\n    base: ' + externalUrl + '\nsections:\n  - name: Screens\n    items:\n      - label: Fixture\n        src: fixture.html\n');
  function page(color) { return '<html><body style="margin:0;background:' + color + ';font:32px sans-serif"><h1>Warm capture</h1><div style="height:1500px">Scroll fixture</div></body></html>'; }
  fs.writeFileSync(path.join(root, 'fixture.html'), page('#def'));
  var running = await server.start({ root: root, capture: helper, eagerCapture: true });
  var base = new URL(running.url).origin;
  var timings = [];
  try {
    await helper.warm(base);
    var payload = { url: base + '/fixture.html', width: 960, height: 720, revision: '1', scroll: { x: 0, y: 0 }, markup: '' };
    await helper.prepare(base, payload);
    var child = helper.child;
    for (var i = 0; i < 5; i++) {
      var start = performance.now(); var png = await helper.capture(base, payload);
      timings.push(performance.now() - start);
      assert.equal(png.readUInt32BE(16), 960); assert.equal(png.readUInt32BE(20), 720);
      fs.writeFileSync(path.join(root, 'warm.png'), png);
    }
    assert.equal(helper.child, child, 'A warm capture must reuse the process');
    var before = png;
    fs.writeFileSync(path.join(root, 'fixture.html'), page('#fed'));
    payload.revision = '2';
    await helper.prepare(base, payload);
    var after = await helper.capture(base, payload);
    assert.notDeepEqual(after, before, 'Reload must not capture the previous document');
    fs.writeFileSync(path.join(root, 'reloaded.png'), after);
    payload.width = 393; payload.scroll.y = 140;
    await helper.prepare(base, payload);
    var narrow = await helper.capture(base, payload);
    assert.equal(narrow.readUInt32BE(16), 393);
    fs.writeFileSync(path.join(root, 'narrow-scrolled.png'), narrow);
    var post = async function (route, data) {
      var response = await fetch(base + route, { method: 'POST', headers: { 'Content-Type': 'application/json' }, body: JSON.stringify(data) });
      var result = await response.json(); assert.equal(response.status, 200, JSON.stringify(result)); return result;
    };
    var remote = { url: externalUrl, width: 960, height: 720, revision: 'remote-1', anchors: [{ x: 20, y: 40 }], name: 'implementation' };
    await post(server.CAPTURE_PAGE_PREPARE_PATH, remote);
    var preparedLoads = loads;
    var result = await post(server.CAPTURE_PAGE_PATH, remote);
    assert.ok(fs.existsSync(path.join(root, result.file)));
    assert.equal(loads, preparedLoads, 'Warm implementation capture must not navigate again');
    assert.ok(result.targets && result.targets[0], 'Implementation handoff must still describe marked elements');
    assert.equal(lazyLoads, 0, 'Offscreen lazy images must not block preparation or capture');
    remote.scroll = { x: 0, y: 19980 };
    remote.anchors = [{ x: 20, y: 40 }];
    var scrolled = await post(server.CAPTURE_PAGE_PATH, remote);
    assert.equal(lazyLoads, 1, 'Scrolling the image into view must load it before capture');
    assert.equal(scrolled.targets[0], 'img', 'Handoff targets must use the requested scroll position');
    await helper.prepare(base, payload);
    child.kill('SIGKILL');
    await until(function () { return helper.child && helper.child !== child; });
    await helper.queue;
    var info = await helper.send({ method: 'info' });
    assert.equal(info.visible, false); assert.equal(info.dockVisible, false);
    assert.equal(info.initialDockVisible, false); assert.equal(info.debuggerAttached, false);
    assert.equal(info.url, base + '/_workbench/capture.html');
    var recovered = await post(server.CAPTURE_PATH, Object.assign({}, payload, { name: 'recovered' }));
    assert.ok(fs.existsSync(path.join(root, recovered.file)));
    var report = { root: root, workbench: running.url, timings: timings, info: info, externalLoads: loads, recovered: true };
    fs.writeFileSync(path.join(root, 'results.json'), JSON.stringify(report, null, 2));
    console.log(JSON.stringify(report, null, 2));
  } finally {
    await running.close();
    await new Promise(function (resolve) { external.close(resolve); });
  }
}
main().catch(function (error) { console.error(error); process.exitCode = 1; });
