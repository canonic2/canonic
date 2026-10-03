/* Native iOS Simulator video
   --------------------------
   ScreenCaptureKit produces low-latency H.264 in a small ad-hoc-signed helper.
   This manager builds it once, launches it under the stable host application's
   privacy identity, and fans its frames to authorized loopback clients. */
var childProcess = require('node:child_process');
var crypto = require('node:crypto');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');

var SOURCE = path.join(__dirname, 'simulator-stream-demo', 'Capture.swift');
var PLIST = path.join(__dirname, 'simulator-stream-demo', 'Info.plist');

function run(file, args) {
  return new Promise(function (resolve, reject) {
    childProcess.execFile(file, args, { encoding: 'utf8', maxBuffer: 4 * 1024 * 1024 }, function (error, stdout, stderr) {
      if (error) return reject(new Error(String(stderr || error.message || error).trim()));
      resolve(stdout);
    });
  });
}

async function prepare(storage) {
  if (process.platform !== 'darwin') throw new Error('Native Simulator streaming requires macOS.');
  storage = storage || path.join(os.tmpdir(), 'canonic-simulator-stream');
  var app = path.join(storage, 'Canonic Simulator Stream.app');
  var contents = path.join(app, 'Contents');
  var executable = path.join(contents, 'MacOS', 'canonic-simulator-stream');
  var stamp = path.join(storage, 'runtime.json');
  var hash = crypto.createHash('sha256')
    .update(await fs.promises.readFile(SOURCE)).update(await fs.promises.readFile(PLIST)).digest('hex');
  try {
    var current = JSON.parse(await fs.promises.readFile(stamp, 'utf8'));
    if (current.sha256 === hash && fs.existsSync(executable)) return executable;
  } catch (_) {}

  await fs.promises.mkdir(path.dirname(executable), { recursive: true });
  await run('xcrun', [
    'swiftc', '-O', '-swift-version', '5', '-parse-as-library', SOURCE, '-o', executable,
    '-framework', 'ScreenCaptureKit', '-framework', 'VideoToolbox',
    '-framework', 'CoreMedia', '-framework', 'CoreVideo', '-framework', 'CoreGraphics', '-framework', 'AppKit',
  ]);
  await fs.promises.copyFile(PLIST, path.join(contents, 'Info.plist'));
  await run('codesign', ['--force', '--sign', '-', app]);
  await fs.promises.writeFile(stamp, JSON.stringify({ sha256: hash }) + '\n');
  return executable;
}

function websocketFrame(payload) {
  var length = payload.length;
  var header;
  if (length < 126) {
    header = Buffer.from([0x82, length]);
  } else if (length <= 0xffff) {
    header = Buffer.allocUnsafe(4);
    header[0] = 0x82; header[1] = 126; header.writeUInt16BE(length, 2);
  } else {
    header = Buffer.allocUnsafe(10);
    header[0] = 0x82; header[1] = 127; header.writeBigUInt64BE(BigInt(length), 2);
  }
  return Buffer.concat([header, payload]);
}

function helperError(stderr, fallback) {
  var lines = String(stderr || '').trim().split(/\r?\n/).filter(Boolean);
  return String(lines.pop() || fallback).replace(/^Simulator stream failed:\s*/, '');
}

function Manager(options) {
  this.options = options || {};
  this.child = null;
  this.starting = null;
  this.udid = null;
  this.codec = null;
  this.token = null;
  this.clients = new Set();
  this.httpClients = new Set();
  this.pending = Buffer.alloc(0);
  this.latestPacket = null;
  this.generation = 0;
}

Manager.prototype.start = function (request) {
  var codec = request.codec === 'jpeg' ? 'jpeg' : 'h264';
  if (this.child && this.udid === request.udid && this.codec === codec && !this.child.killed) {
    return Promise.resolve({ token: this.token, source: request.source, codec: codec });
  }
  if (this.starting && this.udid === request.udid && this.codec === codec) return this.starting;
  this.stop();
  var generation = ++this.generation;
  this.udid = request.udid;
  this.codec = codec;
  this.token = crypto.randomBytes(24).toString('hex');
  var self = this;
  var preparing = (this.options.prepare || prepare)(this.options.storage).then(function (file) {
    if (generation !== self.generation) throw new Error('Simulator stream start was superseded.');
    return new Promise(function (resolve, reject) {
      /* Launch directly so macOS attributes Screen Recording to the stable
         host application (VS Code in the extension), not this rebuilt ad-hoc
         helper. The name only makes that ownership explicit in its error. */
      var args = request.source ? [request.source] : [];
      if (codec === 'jpeg') args.unshift('--jpeg');
      var child = (self.options.spawn || childProcess.spawn)(file, args, {
        stdio: ['ignore', 'pipe', 'pipe'],
        windowsHide: true,
        env: Object.assign({}, process.env, {
          CANONIC_SCREEN_CAPTURE_OWNER:
            self.options.permissionOwner || 'the application running Workbench',
        }),
      });
      self.child = child;
      var stderr = '';
      var settled = false;
      var timer = setTimeout(function () { fail(new Error('Simulator stream startup timed out' + (stderr ? ': ' + stderr : ''))); }, 20000);
      function fail(error) {
        if (!settled) { settled = true; clearTimeout(timer); reject(error); }
      }
      child.once('error', fail);
      child.once('exit', function (code, signal) {
        var current = self.child === child;
        if (current) self.child = null;
        fail(new Error(helperError(stderr, 'Simulator stream stopped (' + (signal || code) + ')')));
        if (current) {
          self.clients.forEach(function (socket) { socket.destroy(); });
          self.clients.clear();
        }
      });
      child.stderr.setEncoding('utf8');
      child.stderr.on('data', function (chunk) {
        stderr = (stderr + chunk).slice(-4096);
        if (!settled && /Streaming .+ at \d+x\d+, \d+ fps/.test(stderr)) {
          settled = true;
          clearTimeout(timer);
          resolve({ token: self.token, source: request.source, codec: codec });
        }
      });
      child.stdout.on('data', function (chunk) { self.frames(chunk); });
    });
  });
  this.starting = preparing.finally(function () {
    if (self.starting === preparing || self.starting === wrapped) self.starting = null;
  });
  var wrapped = this.starting;
  return wrapped;
};

Manager.prototype.frames = function (chunk) {
  this.pending = Buffer.concat([this.pending, chunk]);
  while (this.pending.length >= 13) {
    var length = this.pending.readUInt32BE(0);
    if (this.pending.length < 13 + length) return;
    var key = this.pending[12] === 1;
    var packet = Buffer.concat([Buffer.from([key ? 1 : 0]), this.pending.subarray(4, 12), this.pending.subarray(13, 13 + length)]);
    if (key) this.latestPacket = packet;
    var wire = websocketFrame(packet);
    this.clients.forEach(function (socket) {
      if (socket.destroyed || (socket.waitingForKeyframe && !key)) return;
      socket.waitingForKeyframe = false;
      if (!socket.write(wire)) socket.waitingForKeyframe = true;
    });
    var record = Buffer.allocUnsafe(4 + packet.length);
    record.writeUInt32BE(packet.length, 0);
    packet.copy(record, 4);
    this.httpClients.forEach(function (response) {
      if (response.destroyed || response.waitingForFrame) return;
      if (!response.write(record)) response.waitingForFrame = true;
    });
    this.pending = this.pending.subarray(13 + length);
  }
};

Manager.prototype.accept = function (req, socket) {
  var target;
  try { target = new URL(req.url, 'http://127.0.0.1'); } catch (_) { return false; }
  if (target.pathname !== '/_workbench/simulator/stream' || target.searchParams.get('token') !== this.token || !this.child) return false;
  var key = req.headers['sec-websocket-key'];
  if (!key) return false;
  var accept = crypto.createHash('sha1').update(key + '258EAFA5-E914-47DA-95CA-C5AB0DC85B11').digest('base64');
  socket.write(['HTTP/1.1 101 Switching Protocols', 'Upgrade: websocket', 'Connection: Upgrade', 'Sec-WebSocket-Accept: ' + accept, '', ''].join('\r\n'));
  socket.waitingForKeyframe = true;
  this.clients.add(socket);
  if (this.latestPacket) {
    socket.waitingForKeyframe = false;
    if (!socket.write(websocketFrame(this.latestPacket))) socket.waitingForKeyframe = true;
  }
  var self = this;
  function remove() { self.clients.delete(socket); }
  socket.on('close', remove);
  socket.on('error', remove);
  return true;
};

Manager.prototype.acceptHttp = function (req, res) {
  var target;
  try { target = new URL(req.url, 'http://127.0.0.1'); } catch (_) { return false; }
  if (target.pathname !== '/_workbench/simulator/stream' || target.searchParams.get('token') !== this.token || !this.child) return false;
  res.writeHead(200, {
    'Content-Type': 'application/octet-stream',
    'Cache-Control': 'no-store',
    'X-Content-Type-Options': 'nosniff',
  });
  res.flushHeaders();
  res.waitingForFrame = false;
  res.on('drain', function () { res.waitingForFrame = false; });
  this.httpClients.add(res);
  if (this.latestPacket) {
    var record = Buffer.allocUnsafe(4 + this.latestPacket.length);
    record.writeUInt32BE(this.latestPacket.length, 0);
    this.latestPacket.copy(record, 4);
    if (!res.write(record)) res.waitingForFrame = true;
  }
  var self = this;
  function remove() { self.httpClients.delete(res); }
  req.on('close', remove);
  res.on('close', remove);
  res.on('error', remove);
  return true;
};

Manager.prototype.stop = function () {
  this.generation++;
  if (this.child && !this.child.killed) this.child.kill('SIGTERM');
  this.child = null;
  this.starting = null;
  this.udid = null;
  this.codec = null;
  this.token = null;
  this.pending = Buffer.alloc(0);
  this.latestPacket = null;
  this.clients.forEach(function (socket) { socket.destroy(); });
  this.clients.clear();
  this.httpClients.forEach(function (response) { response.end(); });
  this.httpClients.clear();
};

Manager.prototype.close = function () { this.stop(); return Promise.resolve(); };

module.exports = {
  create: function (options) { return new Manager(options); },
  Manager: Manager,
  prepare: prepare,
  websocketFrame: websocketFrame,
  helperError: helperError,
};
