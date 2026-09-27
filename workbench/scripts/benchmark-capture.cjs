/* Native capture profiling with current helper sources in an isolated runtime.
   node packages/workbench/scripts/benchmark-capture.cjs [project page width height]
   With no arguments, compare identical fixtures with CSS filters toggled.
   Reports and PNGs stay in a temporary directory; installed helpers are untouched. */
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var assert = require('node:assert/strict');
var runtime = require('../capture-runtime');
var capture = require('../electron-capture');
var server = require('../server');

function dimensions(buffer, format) {
  if (format !== 'jpeg') return [buffer.readUInt32BE(16), buffer.readUInt32BE(20)];
  assert.equal(buffer.readUInt16BE(0), 0xffd8, 'Expected JPEG');
  for (var at = 2; at < buffer.length;) {
    assert.equal(buffer[at++], 0xff);
    var marker = buffer[at++];
    var length = buffer.readUInt16BE(at);
    if ([0xc0, 0xc1, 0xc2].includes(marker)) return [buffer.readUInt16BE(at + 5), buffer.readUInt16BE(at + 3)];
    at += length;
  }
  throw new Error('JPEG has no dimensions');
}

function fixture(effect) {
  var cards = Array.from({ length: 240 }, function (_, i) {
    return '<article><h2>Panel ' + i + '</h2><p>Layers, gradients and fine text</p><div class="bar"></div></article>';
  }).join('');
  return '<!doctype html><meta charset="utf-8"><style>' +
    'body{margin:0;color:white;background:#131c32;font:16px system-ui}' +
    '.glow{position:fixed;inset:-100px;background:repeating-conic-gradient(#578cff 0% 10%,#ff5698 10% 20%,#23413a 20% 30%);' +
    (effect === 'filter' ? 'filter:blur(80px);' : '') + '}' +
    'main{position:relative;display:grid;grid-template-columns:repeat(3,1fr);gap:16px;padding:24px}' +
    'article{padding:20px;border:1px solid #ffffff66;border-radius:24px;background:#11223366;' +
    (effect === 'backdrop' ? 'backdrop-filter:blur(40px);' : '') + '}' +
    '.bar{height:50px;background:linear-gradient(100deg,#ffb670,#8064ff);border-radius:20px}' +
    '</style><div class="glow"></div><main>' + cards + '</main>';
}

async function main() {
  assert.ok(runtime.available(), 'Bundle the native runtime first');
  var work = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-capture-benchmark-'));
  console.log('Capture benchmark: ' + work);
  var project = process.argv[2] ? path.resolve(process.argv[2]) : path.join(work, 'fixture');
  var cases;
  if (process.argv[2]) {
    assert.ok(process.argv[3], 'Pass a page path relative to the project');
    cases = [{ page: process.argv[3], width: Number(process.argv[4]) || 1440, height: Number(process.argv[5]) || 1000 }];
  } else {
    fs.mkdirSync(project);
    fs.writeFileSync(path.join(project, 'workbench.yaml'), 'name: Capture benchmark\nsections: []\n');
    cases = [];
    ['none', 'filter', 'backdrop'].forEach(function (effect) {
      fs.writeFileSync(path.join(project, effect + '.html'), fixture(effect));
      [[393, 852], [1440, 1000]].forEach(function (size) {
        cases.push({ page: effect + '.html', width: size[0], height: size[1] });
      });
    });
  }
  // Unpack into this run's own directory before replacing the bundled source.
  var executable = await runtime.prepare(path.join(work, 'runtime'));
  var appDir = process.platform === 'darwin'
    ? path.resolve(executable, '../../Resources/app')
    : path.join(path.dirname(executable), 'resources/app');
  var source = fs.readFileSync(path.join(__dirname, '../capture-helper/main.cjs'), 'utf8');
  var mode = process.env.CANONIC_BENCH_CAPTURE || 'native';
  var samples = Number(process.env.CANONIC_BENCH_SAMPLES || 6);
  assert.ok(Number.isInteger(samples) && samples > 0 && samples <= 100, 'Invalid sample count');
  var format = process.env.CANONIC_BENCH_FORMAT || 'png';
  var resizeQuality = process.env.CANONIC_BENCH_RESIZE || 'better';
  assert.ok(['best', 'better', 'good'].includes(resizeQuality), 'Invalid resize quality');
  source = source.replace("quality: format === 'jpeg' ? 'better' : 'best'", "quality: format === 'jpeg' ? '" + resizeQuality + "' : 'best'");
  assert.ok(['png', 'jpeg'].includes(format), 'CANONIC_BENCH_FORMAT must be png or jpeg');
  assert.ok(['native', 'cdp', 'dom'].includes(mode), 'CANONIC_BENCH_CAPTURE must be native, cdp or dom');
  if (mode === 'cdp') {
    // Experiment only in the isolated runtime. Target the helper, never VS Code.
    source += `
      png = async function (timings, format) {
        var start = performance.now();
        if (!win.webContents.debugger.isAttached()) win.webContents.debugger.attach('1.3');
        var scale = await win.webContents.executeJavaScript('window.devicePixelRatio');
        var result = await win.webContents.debugger.sendCommand('Page.captureScreenshot', {
          format: format || 'png', quality: 90, fromSurface: true, captureBeyondViewport: false, optimizeForSpeed: true,
          clip: { x: 0, y: 0, width: width, height: height, scale: 1 / scale }
        });
        if (timings) timings.captureAndEncodeMs = performance.now() - start;
        return result.data;
      };
    `;
  }
  if (mode === 'dom') {
    var renderer = fs.readFileSync(path.join(__dirname, '../workbench/modern-screenshot.js'), 'utf8');
    var domStyles = process.env.CANONIC_BENCH_DOM_STYLES === 'resolved'
      ? 'includeStyleProperties:Array.from(getComputedStyle(document.documentElement)).filter(function(name) { return name.indexOf("--") !== 0; }),' : '';
    source += `
      png = async function (timings, format) {
        var start = performance.now();
        await win.webContents.executeJavaScript(${JSON.stringify(renderer)} + '\\n;true');
        await win.webContents.executeJavaScript(\
          'window.__captureStages = {}; window.__captureStarts = {};' +
          'console.time = function(key) { window.__captureStarts[key] = performance.now(); };' +
          'console.timeEnd = function(key) { window.__captureStages[key] = performance.now() - window.__captureStarts[key]; }; true');
        var data = await win.webContents.executeJavaScript(\
          'modernScreenshot.domToBlob(document.body, {${domStyles}debug:true,width:' + width + ',height:' + height + ',scale:1,quality:0.9,type:"image/' + (format || 'png') + '"})' +
          '.then(function(blob) { return new Promise(function(resolve, reject) {' +
          'var reader = new FileReader(); reader.onload = function() { resolve(reader.result.split(",")[1]); };' +
          'reader.onerror = reject; reader.readAsDataURL(blob); }); })');
        if (timings) {
          timings.captureAndEncodeMs = performance.now() - start;
          timings.domStages = await win.webContents.executeJavaScript('window.__captureStages');
        }
        return data;
      };
    `;
  }
  fs.writeFileSync(path.join(appDir, 'main.cjs'), source);
  var helper = capture.create({ executable: executable });
  var running;
  var report = { mode: mode, format: format, resizeQuality: resizeQuality, project: project, cases: [] };
  try {
    running = await server.start({ root: project, capture: helper, eagerCapture: false });
    var base = new URL(running.url).origin;
    await helper.warm(base);
    report.info = await helper.send({ method: 'info' });
    for (var item of cases) {
      var payload = { url: new URL(item.page, base + '/').href, width: item.width, height: item.height,
        revision: 'benchmark-' + report.cases.length, scroll: { x: 0, y: 0 }, markup: '', format: format };
      var entry = Object.assign({ captures: [] }, item);
      report.cases.push(entry);
      var start = performance.now();
      try {
        await helper.prepare(base, payload);
        entry.prepareMs = performance.now() - start;
        for (var i = 0; i < samples; i++) {
          // Include a capture after the hidden renderer has been idle.
          if (i === 5) await new Promise(function (resolve) { setTimeout(resolve, 1200); });
          start = performance.now();
          var result = await helper.serial(function () {
            return helper.send({ method: 'capture', base: base, payload: payload, profile: true });
          });
          var buffer = Buffer.from(result.data, 'base64');
          var totalMs = performance.now() - start;
          assert.deepEqual(dimensions(buffer, format), [item.width, item.height]);
          entry.captures.push(Object.assign({ totalMs: totalMs, afterIdle: i === 5 }, result.timings));
          fs.writeFileSync(path.join(work, 'results.json'), JSON.stringify(report, null, 2));
          if (i === 0) fs.writeFileSync(path.join(work, 'capture-' + (report.cases.length - 1) + (format === 'jpeg' ? '.jpg' : '.png')), buffer);
        }
        if (!process.argv[2]) {
          // Save only into our temporary fixtures, never a selected project.
          start = performance.now();
          var response = await fetch(base + '/_workbench/capture', {
            method: 'POST', headers: { 'Content-Type': 'application/json' },
            body: JSON.stringify(Object.assign({}, payload, { name: 'benchmark' })),
          });
          var saved = await response.json();
          entry.saveRoundTripMs = performance.now() - start;
          assert.ok(response.ok && saved.ok, JSON.stringify(saved));
          assert.deepEqual(dimensions(fs.readFileSync(path.join(project, saved.file)), format), [item.width, item.height]);
        }
      } catch (error) { entry.error = error.message; }
      fs.writeFileSync(path.join(work, 'results.json'), JSON.stringify(report, null, 2));
      console.log(JSON.stringify(entry));
    }
  } finally {
    if (running) await running.close();
    else await helper.close();
    await fs.promises.rm(path.join(work, 'runtime'), { recursive: true, force: true });
  }
  console.log('Report: ' + path.join(work, 'results.json'));
  if (report.cases.some(function (entry) { return entry.error; })) process.exitCode = 1;
}
main().catch(function (error) { console.error(error); process.exitCode = 1; });
