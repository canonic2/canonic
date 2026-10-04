/* A private, background renderer. Only the parent process can issue commands;
   preview JavaScript has neither Node access nor an IPC bridge. */
var electron = require('electron');
var readline = require('node:readline');
var app = electron.app;
var win;
var loaded = null;
var pageRevision = null;
var width = 960;
var height = 720;
var queue = Promise.resolve();
var primed = null;
var initialDockVisible = process.platform === 'darwin' ? app.dock.isVisible() : false;

if (!process.env.CANONIC_CAPTURE_PROFILE) app.exit(1);
app.setPath('userData', process.env.CANONIC_CAPTURE_PROFILE);
app.setPath('sessionData', process.env.CANONIC_CAPTURE_PROFILE);
// Keep the OS media preferences. Live mirrors transfer animation phases;
// forcing reduced motion here would select different CSS from the preview.

function httpUrl(value) {
  var url = new URL(value);
  if (!/^https?:$/.test(url.protocol)) throw new Error('Capture requires an HTTP page');
  return url;
}

async function navigate(url, reload) {
  httpUrl(url);
  if (!reload && loaded === url) return;
  loaded = null;
  primed = null;
  await win.loadURL(url);
  loaded = url;
}

function sameStorybookPreview(from, to) {
  try {
    var current = new URL(from);
    var next = new URL(to);
    return current.origin === next.origin && current.pathname === next.pathname &&
      current.pathname.replace(/\/+$/, '').endsWith('/iframe.html');
  } catch (_) { return false; }
}

async function navigatePage(request) {
  var url = request.payload.url;
  if (request.payload.reuse === 'storybook' && sameStorybookPreview(loaded, url)) {
    var changed = loaded !== url;
    if (changed) await win.webContents.executeJavaScript(request.switchStory);
    loaded = url;
    primed = null;
    return changed;
  }
  await navigate(url, !request.payload.revision || pageRevision !== request.payload.revision);
  return true;
}

function resize(payload) {
  if (!Number.isInteger(payload.width) || !Number.isInteger(payload.height) ||
      payload.width < 1 || payload.height < 1 || payload.width > 8192 || payload.height > 8192) {
    throw new Error('Invalid capture dimensions');
  }
  if (width === payload.width && height === payload.height) return;
  width = payload.width;
  height = payload.height;
  primed = null;
  win.setContentSize(width, height);
}

async function restorePointer(payload) {
  if (!payload.mirror) return;
  var point = payload.mirror.pointer;
  win.webContents.sendInputEvent({ type: 'mouseMove',
    x: point ? Math.round(point.x) : width + 1, y: point ? Math.round(point.y) : height + 1 });
  await win.webContents.executeJavaScript('new Promise(function (resolve) { requestAnimationFrame(function () { requestAnimationFrame(resolve); }); })');
}

async function prepare(request) {
  var base = httpUrl(request.base);
  if (base.protocol !== 'http:' || !['127.0.0.1', 'localhost'].includes(base.hostname) ||
      httpUrl(request.payload.url).origin !== base.origin) throw new Error('Capture must use its workbench origin');
  resize(request.payload);
  await navigate(new URL('/_workbench/capture.html', base).href);
  var details = await win.webContents.executeJavaScript('window.wbCapture.prepare(' + JSON.stringify(request.payload) + ')');
  await restorePointer(request.payload);
  return details;
}

async function preparePage(request) {
  resize(request.payload);
  await navigate(request.payload.url, !request.payload.revision || pageRevision !== request.payload.revision);
  pageRevision = request.payload.revision;
  if (request.inject) await win.webContents.executeJavaScript(request.inject + '\n;true');
  // Interactive screenshots use the established scroll-safe path: restore
  // before loading visible assets, then once more immediately before capture.
  return win.webContents.executeJavaScript(`(async function () {
    var scroll = ${JSON.stringify(request.payload.scroll || { x: 0, y: 0 })};
    function scrollNow() {
      var root = document.documentElement;
      var behavior = root.style.getPropertyValue('scroll-behavior');
      var priority = root.style.getPropertyPriority('scroll-behavior');
      root.style.setProperty('scroll-behavior', 'auto', 'important');
      window.scrollTo(scroll.x, scroll.y);
      if (behavior) root.style.setProperty('scroll-behavior', behavior, priority);
      else root.style.removeProperty('scroll-behavior');
    }
    scrollNow();
    if (document.fonts) await document.fonts.ready;
    await Promise.all(Array.from(document.images).filter(function (img) {
      var box = img.getBoundingClientRect();
      return box.width > 0 && box.height > 0 && box.right > 0 && box.bottom > 0 &&
        box.left < window.innerWidth && box.top < window.innerHeight;
    }).map(function (img) {
      return img.decode ? img.decode().catch(function () {}) : Promise.resolve();
    }));
    scrollNow();
    return { scroll: {
      requestedX: Number(scroll.x) || 0, requestedY: Number(scroll.y) || 0,
      appliedX: window.scrollX || window.pageXOffset || 0,
      appliedY: window.scrollY || window.pageYOffset || 0
    } };
  })()`);
}

async function prepareExportPage(request) {
  resize(request.payload);
  var changed = await navigatePage(request);
  pageRevision = request.payload.revision;
  if (request.inject) await win.webContents.executeJavaScript(request.inject + '\n;true');
  var details = await win.webContents.executeJavaScript(`(function () {
    var scroll = ${JSON.stringify(request.payload.scroll || { x: 0, y: 0 })};
    var root = document.documentElement;
    var behavior = root.style.getPropertyValue('scroll-behavior');
    var priority = root.style.getPropertyPriority('scroll-behavior');
    root.style.setProperty('scroll-behavior', 'auto', 'important');
    window.scrollTo(scroll.x, scroll.y);
    if (behavior) root.style.setProperty('scroll-behavior', behavior, priority);
    else root.style.removeProperty('scroll-behavior');
    return { scroll: {
      requestedX: Number(scroll.x) || 0, requestedY: Number(scroll.y) || 0,
      appliedX: window.scrollX || window.pageXOffset || 0,
      appliedY: window.scrollY || window.pageYOffset || 0
    } };
  })()`);
  var settle = changed ? request.settle : request.settleFast || request.settle;
  if (settle) await win.webContents.executeJavaScript(settle);
  /* A docs page is captured whole: the window grows to the page's height
     (up to the capture limit), and the page settles again at that size. */
  if (request.payload.fullPage) {
    var full = await win.webContents.executeJavaScript('Math.ceil(Math.max(document.documentElement.scrollHeight, document.body ? document.body.scrollHeight : 0))');
    var grown = Math.max(1, Math.min(8192, Number(full) || height));
    if (grown !== height) {
      resize({ width: width, height: grown });
      await win.webContents.executeJavaScript('new Promise(function (resolve) { requestAnimationFrame(function () { requestAnimationFrame(resolve); }); })');
      if (request.settleFast || request.settle) await win.webContents.executeJavaScript(request.settleFast || request.settle);
    }
  }
  return details;
}

/* The box of the element a reference shows, in window pixels, or null. */
async function elementRect(selector) {
  var box = await win.webContents.executeJavaScript('(function () { var el = document.querySelector(' + JSON.stringify(selector) +
    '); if (!el) return null; var r = el.getBoundingClientRect(); return { x: r.left, y: r.top, width: r.width, height: r.height }; })()');
  if (!box) return null;
  var x = Math.max(0, Math.floor(box.x));
  var y = Math.max(0, Math.floor(box.y));
  return { x: x, y: y, width: Math.max(1, Math.min(width - x, Math.ceil(box.width))), height: Math.max(1, Math.min(height - y, Math.ceil(box.height))) };
}

async function captureBitmap(rect) {
  var bitmap = await win.webContents.capturePage(rect || { x: 0, y: 0, width: width, height: height }, { stayHidden: true });
  if (bitmap.isEmpty()) throw new Error('The capture renderer returned an empty image');
  return bitmap;
}

async function png(timings, format, rect) {
  var start = performance.now();
  var bitmap = await captureBitmap(rect);
  var expected = rect || { width: width, height: height };
  if (timings) timings.readbackMs = performance.now() - start;
  start = performance.now();
  var size = bitmap.getSize();
  if (timings) timings.bitmapSize = size;
  if (size.width !== expected.width || size.height !== expected.height) {
    bitmap = bitmap.resize({ width: expected.width, height: expected.height, quality: format === 'jpeg' ? 'better' : 'best' });
  }
  if (timings) timings.resizeMs = performance.now() - start;
  start = performance.now();
  var data = format === 'jpeg' ? bitmap.toJPEG(90) : bitmap.toPNG();
  if (timings) { timings.encodeMs = performance.now() - start; timings.bytes = data.length; }
  if (format !== 'jpeg' && (data.readUInt32BE(16) !== expected.width || data.readUInt32BE(20) !== expected.height)) throw new Error('Incorrect capture dimensions');
  start = performance.now();
  var encoded = data.toString('base64');
  if (timings) timings.base64Ms = performance.now() - start;
  return encoded;
}

async function prime(key) {
  // Chromium lazily initializes its first readback. Pay that cost during
  // warm-up as well, rather than calling a merely loaded page ready.
  // Warm the compositor readback, without resizing or encoding a throwaway file.
  if (primed !== key) { await captureBitmap(); primed = key; }
}

function viewKey(request) {
  var p = request.payload;
  return JSON.stringify([request.method, p.url, p.revision, p.width, p.height, p.scroll, p.annotations, p.format, p.mirror && p.mirror.revision]);
}

async function handle(request) {
  switch (request.method) {
    case 'warm':
      if (!loaded) await navigate(new URL('/_workbench/capture.html', httpUrl(request.base)).href);
      await prime('warm');
      return {};
    case 'prepare': await prepare(request); await prime(viewKey(request)); return {};
    case 'capture': {
      // Opt-in diagnostics over the private parent pipe; no page-facing API.
      var timings = request.profile ? {} : null;
      var start = performance.now();
      var details = await prepare(request);
      if (timings) timings.prepareMs = performance.now() - start;
      var result = { data: await png(timings, request.payload.format), details: details };
      if (timings) result.timings = timings;
      return result;
    }
    case 'preparePage': await preparePage(request); await prime(viewKey(request)); return {};
    case 'capturePage':
      var pageDetails = await preparePage(request);
      var targets = request.describe ? await win.webContents.executeJavaScript(request.describe) : null;
      try {
        var overlayDetails = await win.webContents.executeJavaScript(request.overlay);
        return { data: await png(null, request.payload.format), targets: targets, details: overlayDetails || pageDetails };
      } finally {
        await win.webContents.executeJavaScript(request.removeOverlay).catch(function () {});
      }
    case 'captureExportPage':
      var exportDetails = await prepareExportPage(request);
      var crop = null;
      if (request.payload.selector) {
        crop = await elementRect(request.payload.selector);
        if (!crop) throw new Error('Nothing on the page matches ' + request.payload.selector);
      }
      try {
        var exportOverlayDetails = await win.webContents.executeJavaScript(request.overlay);
        return { data: await png(null, request.payload.format, crop), details: exportOverlayDetails || exportDetails };
      } finally {
        await win.webContents.executeJavaScript(request.removeOverlay).catch(function () {});
      }
    case 'info': return {
      visible: win.isVisible(), initialDockVisible: initialDockVisible,
      dockVisible: process.platform === 'darwin' ? app.dock.isVisible() : false,
      debuggerAttached: win.webContents.debugger.isAttached(), url: loaded,
      gpu: app.getGPUFeatureStatus(), versions: process.versions,
    };
    default: throw new Error('Unknown capture command');
  }
}

function reply(value) { process.stdout.write(JSON.stringify(value) + '\n'); }
process.stdout.on('error', function () { app.exit(0); });
process.stdin.on('end', function () { app.exit(0); });
app.whenReady().then(function () {
  // LSUIElement in the packaged app prevents even a startup Dock flash.
  if (process.platform === 'darwin') app.setActivationPolicy('accessory');
  win = new electron.BrowserWindow({
    width: width, height: height, useContentSize: true, show: false,
    frame: false, skipTaskbar: true, backgroundColor: '#ffffff', paintWhenInitiallyHidden: true,
    webPreferences: { nodeIntegration: false, contextIsolation: true, sandbox: true, backgroundThrottling: false },
  });
  win.webContents.setWindowOpenHandler(function () { return { action: 'deny' }; });
  win.webContents.session.setPermissionRequestHandler(function (_wc, _permission, callback) { callback(false); });
  win.webContents.session.on('will-download', function (event) { event.preventDefault(); });
  win.webContents.on('will-navigate', function (event, url) {
    try { httpUrl(url); } catch (_) { event.preventDefault(); }
  });
  win.webContents.on('render-process-gone', function () { app.exit(2); });
  var input = readline.createInterface({ input: process.stdin });
  input.on('line', function (line) {
    var request;
    try { request = JSON.parse(line); } catch (_) { app.exit(2); return; }
    if (request.method === 'close') { app.exit(0); return; }
    queue = queue.then(function () { return handle(request); }).then(function (result) {
      reply({ id: request.id, result: result });
    }, function (error) { reply({ id: request.id, error: String(error.message || error) }); });
  });
  reply({ ready: true });
}).catch(function () { app.exit(2); });
