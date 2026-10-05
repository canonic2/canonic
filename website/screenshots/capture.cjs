/* Regenerates the site's screenshots from the Acme fixture beside this file.
   Starts the extension's server on the fixture, drives headless Chrome through
   the extension's development DevTools driver, and writes JPEGs into
   ../public/images.

   It also measures the workbench's four regions in the overview and writes
   them to ../src/data/screenshots.json, where the page draws the numbered
   outlines, so the numbers stay on the page list, top bar, canvas, and
   toolbar. For the controls section it crops each group of controls at high
   resolution and records the strip there too: each crop, with a hotspot over
   every control. It first checks that the page's control list names the same
   controls in the same order, since the hotspots take their captions from
   that list.

   node packages/website/screenshots/capture.cjs

   Needs Chrome, Chromium, or Edge (set CHROME_PATH to pick one). */
var cp = require('node:child_process');
var fs = require('node:fs');
var path = require('node:path');
var EXTENSION = path.resolve(__dirname, '../../workbench');
var chrome = require(path.join(EXTENSION, 'scripts/chrome.cjs'));
var FIXTURE = path.join(__dirname, 'fixture');
var OUT = path.resolve(__dirname, '../public/images');
var PAGE = path.resolve(__dirname, '../src/pages/index.astro');
var DATA = path.resolve(__dirname, '../src/data/screenshots.json');
// The overview is a full desktop window. The state and annotations shots use
// a narrower window, which the canvas zooms to fit.
var W = 1440, SIDE = 1120, H = 860, SCALE = 2;
// Control crops render at 4x and are shown at ZOOM times their CSS size.
var CONTROLS_SCALE = 4, ZOOM = 1.25;
var CONTROLS = [
  { name: 'actions', selector: '#actionsToggle', alt: 'The Actions switch, switched off, enlarged.' },
  { name: 'page', selector: '.wb-topbar-right', alt: 'The size switcher and the page’s actions, enlarged: four sizes, then Reload, Open the source, Copy reference, Open on its own, and More.' },
  { name: 'annotations', selector: '.wb-annotation-tools', alt: 'The annotation tools from the toolbar under the canvas, enlarged: Select, Scribble, Arrow, Shapes, Text, Comment, Undo, Clear annotations, and Save screenshot.' },
  { name: 'view', selector: '#viewControls', alt: 'The view controls, enlarged: Recenter view, Zoom out, the zoom level, and Zoom in.' },
];

function wait(ms) { return new Promise(function (r) { setTimeout(r, ms); }); }

function serve() {
  return new Promise(function (resolve, reject) {
    var server = cp.spawn(process.execPath, [path.join(EXTENSION, 'server.js'), FIXTURE], { stdio: ['ignore', 'pipe', 'inherit'] });
    var out = '';
    server.on('error', reject);
    server.on('exit', function (code) { reject(new Error('The workbench server exited with ' + code)); });
    server.stdout.on('data', function (chunk) {
      out += chunk;
      var match = /workbench on (http:\/\/\S+)/.exec(out);
      if (match) resolve({ url: match[1], process: server });
    });
  });
}

async function main() {
  var server = await serve();
  var browser = new chrome.Browser();
  try {
    await browser.launch();
    var t = await browser.createTarget();
    async function open(hash, width, scale) {
      await t.send('Emulation.setDeviceMetricsOverride', { width: width, height: H, deviceScaleFactor: scale || SCALE, mobile: false });
      // A same-address navigation fires no load event, so start from blank.
      t.pageUrl = null;
      await t.navigate('about:blank');
      await t.navigate(server.url + hash);
      await wait(1800);
    }
    async function shot(name) {
      var r = await t.send('Page.captureScreenshot', { format: 'jpeg', quality: 86 });
      fs.writeFileSync(path.join(OUT, name), Buffer.from(r.data, 'base64'));
      console.log('wrote public/images/' + name);
    }
    function mouse(type, x, y) {
      return t.send('Input.dispatchMouseEvent', { type: type, x: x, y: y, button: 'left', buttons: type === 'mouseReleased' ? 0 : 1, clickCount: 1 });
    }
    async function stroke(points) {
      await mouse('mousePressed', points[0][0], points[0][1]);
      for (var i = 1; i < points.length; i++) await mouse('mouseMoved', points[i][0], points[i][1]);
      var last = points[points.length - 1];
      await mouse('mouseReleased', last[0], last[1]);
      await wait(150);
    }
    function line(x1, y1, x2, y2) {
      var points = [];
      for (var i = 0; i <= 8; i++) points.push([x1 + (x2 - x1) * i / 8, y1 + (y2 - y1) * i / 8]);
      return stroke(points);
    }
    async function tool(selector) {
      await t.evaluate('document.querySelector(' + JSON.stringify(selector) + ').click()');
      await wait(100);
    }
    // An element's box inside the preview, in workbench coordinates. The
    // canvas may be zoomed, so the page's own pixels are scaled to it.
    function rectOf(selector) {
      return t.evaluate('(function(){var f=document.getElementById("frame"),o=f.getBoundingClientRect(),s=o.width/f.offsetWidth,r=f.contentDocument.querySelector(' +
        JSON.stringify(selector) + ').getBoundingClientRect();return {x:o.x+r.x*s,y:o.y+r.y*s,w:r.width*s,h:r.height*s};})()');
    }

    await open('#pages/sign-in.html', W);
    await shot('overview.jpg');
    writeRegions(await t.evaluate('(' + measureRegions + ')()'));

    await open('#pages/sign-in.html', W, CONTROLS_SCALE);
    var page = fs.readFileSync(PAGE, 'utf8'), strip = [];
    for (var g = 0; g < CONTROLS.length; g++) {
      var group = CONTROLS[g];
      var m = await t.evaluate('(' + measureControls + ')(' + JSON.stringify(group.selector) + ')');
      checkLegend(page, group.name, m.controls);
      // Crop to the controls themselves: a group's box can include empty
      // bar space where it stretches.
      var left = Math.min.apply(null, m.controls.map(function (c) { return c.x; }));
      var right = Math.max.apply(null, m.controls.map(function (c) { return c.x + c.w; }));
      var crop = { x: left - 10, y: m.barY + 2, width: right - left + 20, height: m.barH - 4 };
      var png = await t.send('Page.captureScreenshot', { format: 'png', clip: Object.assign({ scale: 1 }, crop) });
      var file = 'controls-' + group.name + '.png';
      fs.writeFileSync(path.join(OUT, file), Buffer.from(png.data, 'base64'));
      console.log('wrote public/images/' + file);
      strip.push(stripGroup(group, file, crop, m.controls));
    }
    writeData('controls', strip);

    await open('#pages/sign-in.html:error@mobile', SIDE);
    await shot('states.jpg');

    await open('#pages/sign-in.html:error@mobile', SIDE);
    var alert = await rectOf('.alert');
    var field = await rectOf('input[type=password]');
    await tool('#shapeTool');
    await line(alert.x - 8, alert.y - 8, alert.x + alert.w + 8, alert.y + alert.h + 8);
    await tool('[data-tool="arrow"]');
    await line(alert.x + alert.w * 0.82, alert.y - 120, alert.x + alert.w * 0.62, alert.y - 14);
    await tool('[data-tool="draw"]');
    var wave = [], y = field.y + field.h + 10;
    for (var i = 0; i <= 40; i++) wave.push([field.x + 6 + (field.w - 12) * i / 40, y + Math.sin(i * 0.9) * 4]);
    await stroke(wave);
    // Leave no annotation selected: back to Select, then click the empty stage.
    await tool('[data-tool="pointer"]');
    await mouse('mousePressed', 400, 700);
    await mouse('mouseReleased', 400, 700);
    await wait(300);
    await shot('annotations.jpg');
    await open('#preview/interactive-button.workbench.ts@laptop', SIDE);
    await t.evaluate(`(async function () {
      var deadline = Date.now() + 8000;
      while (document.getElementById('previewControls').hidden) {
        if (Date.now() > deadline) throw new Error('Workbench preview controls did not become ready');
        await new Promise(resolve => setTimeout(resolve, 50));
      }
      document.getElementById('previewControls').click();
    })()`);
    await wait(200);
    await shot('workbench-previews.jpg');
  } finally {
    await browser.close();
    server.process.removeAllListeners('exit');
    server.process.kill();
  }
}

/* Runs in the workbench. Each region's outline, and a spot for its number
   that sits on the region without covering a control. */
function measureRegions() {
  function box(el) { var r = el.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; }
  var top = box(document.querySelector('.wb-topbar'));
  var crumb = box(document.getElementById('crumb'));
  var center = box(document.querySelector('.wb-topbar-center'));
  var side = box(document.querySelector('.wb-sidebar'));
  var list = document.getElementById('pageList');
  var pages = box(list.lastElementChild || list);
  var canvas = box(document.querySelector('.wb-canvas'));
  var toolbar = box(document.getElementById('toolbar'));
  return {
    width: innerWidth, height: innerHeight,
    regions: [
      { n: 1, name: 'page list', box: side, at: [side.x + side.w / 2, pages.y + pages.h + 40] },
      { n: 2, name: 'top bar', box: top, at: [(crumb.x + crumb.w + center.x) / 2, top.y + top.h / 2] },
      { n: 3, name: 'canvas', box: canvas, at: [canvas.x + 40, canvas.y + canvas.h * 0.22] },
      { n: 4, name: 'toolbar', box: toolbar, at: [toolbar.x - 28, toolbar.y + toolbar.h / 2] },
    ],
  };
}

/* Runs in the workbench. A group of controls: the bar it sits in and its
   visible controls, left to right. The caret beside Rectangle belongs to the
   shape tool. The crop follows whichever bar the group sits in. */
function measureControls(selector) {
  var group = document.querySelector(selector);
  var bar = group.closest('.wb-topbar, .wb-toolbar, .wb-view-controls').getBoundingClientRect();
  var controls = group.matches('button, a') ? [group] : Array.prototype.filter.call(
    group.querySelectorAll('button, a'),
    function (el) { return el.offsetParent && !el.closest('[hidden]') && !el.closest('.wb-menu') && !el.classList.contains('wb-caret'); });
  return {
    barY: bar.y, barH: bar.height,
    controls: controls.map(function (el) {
      var r = el.getBoundingClientRect();
      return { label: el.getAttribute('aria-label') || el.title, x: r.x, w: r.width };
    }),
  };
}

/* The control list is written by hand; its data-label attributes must name
   the measured controls in order, or a hotspot would caption the wrong one. */
function checkLegend(page, name, controls) {
  var list = new RegExp('<dl class="controls" data-group="' + name + '"[^>]*>([\\s\\S]*?)</dl>').exec(page);
  if (!list) throw new Error('index.astro has no control list for the group ' + name);
  var labels = [], item = /data-label="([^"]*)"/g, hit;
  while ((hit = item.exec(list[1]))) labels.push(hit[1]);
  var found = controls.map(function (c) { return c.label; });
  if (labels.join('|') !== found.join('|')) {
    throw new Error('The ' + name + ' control list does not match the workbench.\n  list:      ' + labels.join(', ') + '\n  workbench: ' + found.join(', '));
  }
}

/* One group of the strip: its crop, and a hotspot over each control. The
   hotspot is a little wider than the control, and as tall as the crop. */
function stripGroup(group, file, crop, controls) {
  return {
    name: group.name, src: 'images/' + file,
    width: Math.round(crop.width * ZOOM), height: Math.round(crop.height * ZOOM), alt: group.alt,
    controls: controls.map(function (c) {
      return { label: c.label, left: pct(c.x - 3 - crop.x, crop.width), width: pct(c.w + 6, crop.width) };
    }),
  };
}

function pct(v, of) { return Number((v / of * 100).toFixed(2)); }

function writeRegions(m) {
  var inset = 3;
  writeData('regions', m.regions.map(function (r) {
    var b = r.box;
    return {
      n: r.n, name: r.name,
      box: { left: pct(b.x + inset, m.width), top: pct(b.y + inset, m.height), width: pct(b.w - inset * 2, m.width), height: pct(b.h - inset * 2, m.height) },
      pin: { left: pct(r.at[0], m.width), top: pct(r.at[1], m.height) },
    };
  }));
}

/* The page reads its regions and controls strip from this file. */
function writeData(key, value) {
  var data = JSON.parse(fs.readFileSync(DATA, 'utf8'));
  data[key] = value;
  fs.writeFileSync(DATA, JSON.stringify(data, null, 2) + '\n');
  console.log('wrote src/data/screenshots.json ' + key);
}

main().catch(function (error) { console.error(error); process.exitCode = 1; });
