/* Regenerates the site's screenshots from the Acme fixture beside this file.
   Starts the extension's server on the fixture, drives headless Chrome through
   the extension's own DevTools driver, and writes JPEGs into ../public/images.

   It also measures the workbench's four regions in the overview and writes
   them to ../src/data/screenshots.json, where the page draws the numbered
   outlines, so the numbers stay on the screen list, top bar, canvas, and
   markup bar. For the toolbar section it crops each group of controls at high
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
var capture = require(path.join(EXTENSION, 'capture.js'));
var FIXTURE = path.join(__dirname, 'fixture');
var OUT = path.resolve(__dirname, '../public/images');
var PAGE = path.resolve(__dirname, '../src/pages/index.astro');
var DATA = path.resolve(__dirname, '../src/data/screenshots.json');
// The overview is a full desktop window. The state and markup shots use a
// narrower window, which the canvas zooms to fit.
var W = 1440, SIDE = 1120, H = 860, SCALE = 2;
// Toolbar crops render at 4x and are shown at ZOOM times their CSS size.
var TOOLBAR_SCALE = 4, ZOOM = 1.25;
var TOOLBAR = [
  { name: 'actions', selector: '#actionsToggle', alt: 'The Actions toggle, switched off, enlarged.' },
  { name: 'frame', selector: '.wb-topbar-right', alt: 'The frame and screen controls, enlarged: four frame sizes, then Reload, Open the source, Copy reference, Open on its own, and More.' },
  { name: 'markup', selector: '.wb-markup', alt: 'The markup tools from the bar under the canvas, enlarged: Select, Scribble, Arrow, Shapes, Text, Comment, Undo, Clear markup, and Save screenshot.' },
  { name: 'zoom', selector: '#zoomControl', alt: 'The zoom control, enlarged: Zoom out, the zoom level, and Zoom in.' },
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
  var cap = capture.create({ chromePath: process.env.CHROME_PATH || undefined });
  try {
    await cap.launch();
    var t = cap.target;
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

    await open('#pages/sign-in.html', W, TOOLBAR_SCALE);
    var page = fs.readFileSync(PAGE, 'utf8'), strip = [];
    for (var g = 0; g < TOOLBAR.length; g++) {
      var group = TOOLBAR[g];
      var m = await t.evaluate('(' + measureToolbar + ')(' + JSON.stringify(group.selector) + ')');
      checkLegend(page, group.name, m.controls);
      // Crop to the controls themselves: a group's box can include empty
      // toolbar space where it stretches.
      var left = Math.min.apply(null, m.controls.map(function (c) { return c.x; }));
      var right = Math.max.apply(null, m.controls.map(function (c) { return c.x + c.w; }));
      var crop = { x: left - 10, y: m.barY + 2, width: right - left + 20, height: m.barH - 4 };
      var png = await t.send('Page.captureScreenshot', { format: 'png', clip: Object.assign({ scale: 1 }, crop) });
      var file = 'toolbar-' + group.name + '.png';
      fs.writeFileSync(path.join(OUT, file), Buffer.from(png.data, 'base64'));
      console.log('wrote public/images/' + file);
      strip.push(stripGroup(group, file, crop, m.controls));
    }
    writeData('toolbar', strip);

    await open('#pages/sign-in.html:error@393', SIDE);
    await shot('states.jpg');

    await open('#pages/sign-in.html:error@393', SIDE);
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
    // Leave no mark selected: back to Select, then click the empty stage.
    await tool('[data-tool="pointer"]');
    await mouse('mousePressed', 400, 700);
    await mouse('mouseReleased', 400, 700);
    await wait(300);
    await shot('markup.jpg');
  } finally {
    await cap.close();
    server.process.removeAllListeners('exit');
    server.process.kill();
  }
}

/* Runs in the workbench. Each region's outline, and a spot for its number
   that sits on the region without covering a control. */
function measureRegions() {
  function box(el) { var r = el.getBoundingClientRect(); return { x: r.x, y: r.y, w: r.width, h: r.height }; }
  var top = box(document.querySelector('.wb-topbar'));
  var toggle = box(document.getElementById('actionsToggle'));
  var widths = box(document.querySelector('.wb-widths'));
  var side = box(document.querySelector('.wb-sidebar'));
  var nav = box(document.getElementById('nav').lastElementChild || document.getElementById('nav'));
  var canvas = box(document.querySelector('.wb-canvas'));
  var dock = box(document.getElementById('dock'));
  return {
    width: innerWidth, height: innerHeight,
    regions: [
      { n: 1, name: 'screen list', box: side, at: [side.x + side.w / 2, nav.y + nav.h + 40] },
      { n: 2, name: 'top bar', box: top, at: [(toggle.x + toggle.w + widths.x) / 2, top.y + top.h / 2] },
      { n: 3, name: 'canvas', box: canvas, at: [canvas.x + 40, canvas.y + canvas.h * 0.22] },
      { n: 4, name: 'markup bar', box: dock, at: [dock.x - 28, dock.y + dock.h / 2] },
    ],
  };
}

/* Runs in the workbench. A toolbar group's box and its visible controls, left
   to right. The caret beside Rectangle belongs to the shape tool. The crop
   follows whichever bar the group sits in. */
function measureToolbar(selector) {
  var group = document.querySelector(selector);
  var bar = group.closest('.wb-topbar, .wb-dock, .wb-zoom').getBoundingClientRect();
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
  if (!list) throw new Error('index.astro has no control list for toolbar group ' + name);
  var labels = [], item = /data-label="([^"]*)"/g, hit;
  while ((hit = item.exec(list[1]))) labels.push(hit[1]);
  var found = controls.map(function (c) { return c.label; });
  if (labels.join('|') !== found.join('|')) {
    throw new Error('The ' + name + ' control list does not match the toolbar.\n  list:    ' + labels.join(', ') + '\n  toolbar: ' + found.join(', '));
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

/* The page reads its regions and toolbar strip from this file. */
function writeData(key, value) {
  var data = JSON.parse(fs.readFileSync(DATA, 'utf8'));
  data[key] = value;
  fs.writeFileSync(DATA, JSON.stringify(data, null, 2) + '\n');
  console.log('wrote src/data/screenshots.json ' + key);
}

main().catch(function (error) { console.error(error); process.exitCode = 1; });
