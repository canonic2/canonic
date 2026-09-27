/* ScreenCaptureKit simulator stream proof of concept.
   Builds the Swift helper, relays its length-prefixed Annex B H.264 frames to
   browser WebSocket clients, and serves the WebCodecs canvas demo. */
var childProcess = require('node:child_process');
var crypto = require('node:crypto');
var fs = require('node:fs');
var http = require('node:http');
var path = require('node:path');
var url = require('node:url');

var ROOT = __dirname;
var SOURCE = path.join(ROOT, 'Capture.swift');
var PLIST = path.join(ROOT, 'Info.plist');
var BUILD = path.join(ROOT, '.build');
var APP = path.join(BUILD, 'Canonic Simulator Stream.app');
var CONTENTS = path.join(APP, 'Contents');
var HELPER = path.join(CONTENTS, 'MacOS', 'canonic-simulator-stream');
var PORT = Number(process.env.CANONIC_SIMULATOR_STREAM_PORT || 4587);
var IOS_AUTOMATION_MODULE = url.pathToFileURL(path.join(ROOT, '..', '..', 'src', 'automation', 'ios.mjs')).href;
var wdaFlow = null;
var wdaReady = null;
var wdaScreen = null;
var inputQueue = Promise.resolve();

function compile() {
  fs.mkdirSync(path.dirname(HELPER), { recursive: true });
  var newestSource = Math.max(fs.statSync(SOURCE).mtimeMs, fs.statSync(PLIST).mtimeMs);
  var current = fs.existsSync(HELPER) && fs.statSync(HELPER).mtimeMs >= newestSource;
  if (current) return;
  console.log('Building the ScreenCaptureKit helper…');
  var result = childProcess.spawnSync('xcrun', [
    'swiftc', '-O', '-swift-version', '5', '-parse-as-library', SOURCE, '-o', HELPER,
    '-framework', 'ScreenCaptureKit', '-framework', 'VideoToolbox',
    '-framework', 'CoreMedia', '-framework', 'CoreVideo', '-framework', 'CoreGraphics', '-framework', 'AppKit',
  ], { stdio: 'inherit' });
  if (result.status !== 0) throw new Error('Swift helper build failed.');
  fs.copyFileSync(PLIST, path.join(CONTENTS, 'Info.plist'));
  result = childProcess.spawnSync('codesign', ['--force', '--sign', '-', APP], { stdio: 'inherit' });
  if (result.status !== 0) throw new Error('Swift helper signing failed.');
}

function websocketFrame(payload) {
  var length = payload.length;
  var header;
  if (length < 126) {
    header = Buffer.from([0x82, length]);
  } else if (length <= 0xffff) {
    header = Buffer.allocUnsafe(4);
    header[0] = 0x82;
    header[1] = 126;
    header.writeUInt16BE(length, 2);
  } else {
    header = Buffer.allocUnsafe(10);
    header[0] = 0x82;
    header[1] = 127;
    header.writeBigUInt64BE(BigInt(length), 2);
  }
  return Buffer.concat([header, payload]);
}

compile();

function sendJson(res, statusCode, value) {
  res.writeHead(statusCode, { 'content-type': 'application/json; charset=utf-8', 'cache-control': 'no-store' });
  res.end(JSON.stringify(value));
}

function readJson(req) {
  return new Promise(function (resolve, reject) {
    var chunks = [];
    var size = 0;
    req.on('data', function (chunk) {
      size += chunk.length;
      if (size > 64 * 1024) {
        reject(new Error('Request body is too large.'));
        req.destroy();
        return;
      }
      chunks.push(chunk);
    });
    req.on('end', function () {
      try { resolve(JSON.parse(Buffer.concat(chunks).toString('utf8') || '{}')); }
      catch (error) { reject(new Error('Invalid JSON: ' + error.message)); }
    });
    req.on('error', reject);
  });
}

async function ensureWda() {
  if (wdaReady) return wdaReady;
  wdaReady = (async function () {
    var automation = await import(IOS_AUTOMATION_MODULE);
    var simulator = await automation.firstBootedSimulator();
    if (!simulator) throw new Error('No booted iOS Simulator was found.');
    wdaFlow = automation.createIosAutomationFlow({
      platform: 'simulator',
      udid: simulator.udid,
      artifactDir: path.join('/tmp', 'canonic-simulator-stream-wda'),
      wdaStartupTimeoutMs: 120000,
    });
    await wdaFlow.start({ createSession: true, waitAfterLaunchMs: 0 });
    wdaScreen = await wdaFlow._screenSizeNow();
    return { simulator: simulator, screen: wdaScreen, sessionId: wdaFlow.sessionId };
  })().catch(function (error) {
    wdaReady = null;
    throw error;
  });
  return wdaReady;
}

function validRatio(value) {
  return Number.isFinite(value) && value >= 0 && value <= 1;
}

function queueInput(action) {
  var result = inputQueue.then(action, action);
  inputQueue = result.catch(function () {});
  return result;
}

async function handleWda(body) {
  var ready = await ensureWda();
  if (body.action === 'start') return { ok: true, action: 'start', ...ready };
  if (body.action === 'tap') {
    if (!validRatio(body.x) || !validRatio(body.y)) throw new Error('Tap coordinates must be ratios between 0 and 1.');
    return queueInput(async function () {
      await wdaFlow._tapPoint(
        wdaFlow.sessionId,
        Math.round(wdaScreen.width * body.x),
        Math.round(wdaScreen.height * body.y),
      );
      return { ok: true, action: 'tap', x: body.x, y: body.y };
    });
  }
  if (body.action === 'swipe') {
    if (![body.fromX, body.fromY, body.toX, body.toY].every(validRatio)) {
      throw new Error('Swipe coordinates must be ratios between 0 and 1.');
    }
    return queueInput(async function () {
      await wdaFlow._dragPoint(
        wdaFlow.sessionId,
        Math.round(wdaScreen.width * body.fromX),
        Math.round(wdaScreen.height * body.fromY),
        Math.round(wdaScreen.width * body.toX),
        Math.round(wdaScreen.height * body.toY),
        Math.max(0.05, Math.min(Number(body.duration) || 0.2, 2)),
      );
      return { ok: true, action: 'swipe' };
    });
  }
  throw new Error('Unknown WDA action.');
}

var clients = new Set();
var server = http.createServer(async function (req, res) {
  var pathname = new URL(req.url, 'http://127.0.0.1').pathname;
  if (pathname === '/wda' && req.method === 'POST') {
    try {
      sendJson(res, 200, await handleWda(await readJson(req)));
    } catch (error) {
      sendJson(res, 500, { ok: false, error: error.message });
    }
    return;
  }
  if (pathname === '/' || pathname === '/index.html') {
    res.writeHead(200, { 'content-type': 'text/html; charset=utf-8', 'cache-control': 'no-store' });
    res.end(fs.readFileSync(path.join(ROOT, 'index.html')));
    return;
  }
  res.writeHead(404);
  res.end('Not found');
});

server.on('upgrade', function (req, socket) {
  if (req.url !== '/stream' || !req.headers['sec-websocket-key']) {
    socket.destroy();
    return;
  }
  var accept = crypto.createHash('sha1')
    .update(req.headers['sec-websocket-key'] + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11')
    .digest('base64');
  socket.write([
    'HTTP/1.1 101 Switching Protocols',
    'Upgrade: websocket',
    'Connection: Upgrade',
    'Sec-WebSocket-Accept: ' + accept,
    '', '',
  ].join('\r\n'));
  socket.waitingForKeyframe = true;
  clients.add(socket);
  socket.on('close', function () { clients.delete(socket); });
  socket.on('error', function () { clients.delete(socket); });
});

var title = process.argv.slice(2).join(' ').trim();
var helper = childProcess.spawn(HELPER, title ? [title] : [], { stdio: ['ignore', 'pipe', 'pipe'] });
helper.stderr.setEncoding('utf8');
helper.stderr.on('data', function (chunk) { process.stderr.write(chunk); });
helper.on('exit', function (code, signal) {
  console.error('Capture helper stopped (' + (signal || code) + ').');
  clients.forEach(function (socket) { socket.destroy(); });
  clients.clear();
  helper = null;
});

var pending = Buffer.alloc(0);
helper.stdout.on('data', function (chunk) {
  pending = Buffer.concat([pending, chunk]);
  while (pending.length >= 13) {
    var length = pending.readUInt32BE(0);
    if (pending.length < 13 + length) return;
    var timestamp = pending.subarray(4, 12);
    var key = pending[12] === 1;
    var packet = Buffer.concat([Buffer.from([key ? 1 : 0]), timestamp, pending.subarray(13, 13 + length)]);
    var wire = websocketFrame(packet);
    clients.forEach(function (socket) {
      if (socket.destroyed || (socket.waitingForKeyframe && !key)) return;
      socket.waitingForKeyframe = false;
      if (!socket.write(wire)) socket.waitingForKeyframe = true;
    });
    pending = pending.subarray(13 + length);
  }
});

function stop() {
  if (helper && !helper.killed) helper.kill('SIGTERM');
  if (wdaFlow) wdaFlow.stop({ force: true }).catch(function () {});
  clients.forEach(function (socket) { socket.destroy(); });
  server.close();
}
process.on('SIGINT', stop);
process.on('SIGTERM', stop);

server.listen(PORT, '127.0.0.1', function () {
  console.log('Simulator stream demo: http://127.0.0.1:' + PORT + '/');
});
