/* One bundled renderer for the lifetime of the workbench. Commands share a
   queue; speculative prepares collapse to the newest view. A dead helper is
   replaced and given that view again, even before another screenshot click. */
var childProcess = require('node:child_process');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var readline = require('node:readline');
var scripts = require('./capture-scripts');
var runtime = require('./electron-runtime');

function Capture(options) {
  this.options = options || {};
  this.queue = Promise.resolve();
  this.pending = new Map();
  this.next = 1;
  this.generation = 0;
  this.closed = false;
  this.child = null;
  this.launching = null;
  this.desired = null;
  this.retry = null;
  this.failures = 0;
}

Capture.prototype.recover = function () {
  if (this.closed || !this.desired || this.retry) return;
  var self = this;
  this.retry = setTimeout(function () {
    self.retry = null;
    self.serial(function () { return self.send(self.desired); }).catch(function () { self.recover(); });
  }, Math.min(30000, (this.options.retryDelay || 1000) * Math.pow(2, this.failures++)));
  this.retry.unref();
};

/* `options.diagnostic(level, event, details)`, when given, hears startup timing. */
Capture.prototype.diagnostic = function (level, event, details) {
  if (!this.options.diagnostic) return;
  try { this.options.diagnostic(level, event, details); } catch (_) {}
};

Capture.prototype.launch = function () {
  if (this.closed) return Promise.reject(new Error('Capture helper is closed'));
  if (this.launching) return this.launching;
  var self = this;
  var began = Date.now();
  var spawned;
  this.diagnostic('info', 'capture.helper.starting', {});
  this.launching = Promise.resolve().then(function () {
    return self.options.executable || (self.options.spawn ? 'test-helper' : runtime.prepare(self.options.storage, null, function (event, details) {
      self.diagnostic('info', event, Object.assign({ for: 'capture' }, details));
    }));
  }).then(function (file) {
    if (self.closed) throw new Error('Capture helper is closed');
    spawned = Date.now();
    return self.start(file);
  }).then(function (result) {
    self.diagnostic('info', 'capture.helper.ready', {
      prepareMs: spawned - began, launchMs: Date.now() - spawned, elapsedMs: Date.now() - began,
    });
    return result;
  }).catch(function (error) {
    self.diagnostic('warn', 'capture.helper.failed', { elapsedMs: Date.now() - began, message: String(error.message || error) });
    self.launching = null;
    if (!self.closed) error.code = 'CAPTURE_STARTUP_FAILED';
    self.recover();
    throw error;
  });
  return this.launching;
};

Capture.prototype.start = function (file) {
  var self = this;
  var profile = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-capture-'));
  var env = Object.assign({}, process.env, { CANONIC_CAPTURE_PROFILE: profile });
  delete env.ELECTRON_RUN_AS_NODE;
  delete env.NODE_OPTIONS;
  var child;
  return new Promise(function (resolve, reject) {
    var settled = false;
    var stderr = '';
    var timer;
    function failed(error) {
      if (settled) return;
      settled = true;
      clearTimeout(timer);
      if (self.child === child) { self.child = null; self.launching = null; }
      self.pending.forEach(function (p) { clearTimeout(p.timer); p.reject(error); });
      self.pending.clear();
      function removeProfile() {
        try { fs.rmSync(profile, { recursive: true, force: true, maxRetries: 3 }); } catch (_) {}
      }
      if (child && child.exitCode === null && !child.signalCode) {
        child.once('exit', removeProfile);
        child.kill('SIGKILL');
      } else removeProfile();
      reject(error);
      self.recover();
    }
    try {
      child = (self.options.spawn || childProcess.spawn)(file, [], {
        env: env, windowsHide: true, stdio: ['pipe', 'pipe', 'pipe'],
      });
    } catch (error) { failed(error); return; }
    self.child = child;
    child.once('error', failed);
    child.once('exit', function (code, signal) {
      var error = new Error('Capture helper exited (' + (code === null ? signal || 'unknown signal' : code) + ')' + (stderr ? ': ' + stderr : ''));
      error.code = 'CAPTURE_PROCESS_EXITED';
      failed(error);
    });
    child.stdin.on('error', failed);
    child.stdout.on('error', failed);
    child.stderr.on('data', function (data) { stderr = (stderr + data).slice(-2048); });
    timer = setTimeout(function () { failed(new Error('Capture helper startup timed out')); }, self.options.timeout || 15000);
    readline.createInterface({ input: child.stdout }).on('line', function (line) {
      var message;
      try { message = JSON.parse(line); } catch (_) { return; }
      if (message.ready) { clearTimeout(timer); resolve(); return; }
      var pending = self.pending.get(message.id);
      if (!pending) return;
      self.pending.delete(message.id);
      clearTimeout(pending.timer);
      if (message.error) pending.reject(new Error(message.error));
      else pending.resolve(message.result);
    });
  });
};

Capture.prototype.send = async function (request) {
  await this.launch();
  if (this.closed || !this.child) throw new Error('Capture helper is closed');
  var self = this;
  var id = this.next++;
  var child = this.child;
  return new Promise(function (resolve, reject) {
    var timer = setTimeout(function () {
      // A stuck navigation/evaluation blocks the helper's queue. Replace it;
      // timing out just this request would leave every later one stuck too.
      // The view that hung is not restored on its own: a page that takes
      // this long would only hang the replacement. The workbench asks again
      // when it still wants it, with its own backoff.
      self.pending.delete(id);
      if (self.desired && (self.desired === request || self.desired.payload === request.payload)) self.desired = null;
      reject(new Error('Capture helper timed out during ' + request.method));
      child.kill('SIGKILL');
    }, self.options.commandTimeout || self.options.timeout || 30000);
    self.pending.set(id, { timer: timer, reject: reject, resolve: function (result) {
      self.failures = 0;
      clearTimeout(self.retry);
      self.retry = null;
      resolve(result);
    } });
    child.stdin.write(JSON.stringify(Object.assign({ id: id }, request)) + '\n');
  });
};

Capture.prototype.serial = function (work) {
  var next = this.queue.catch(function () {}).then(work);
  this.queue = next.catch(function () {});
  return next;
};

Capture.prototype.prepareRequest = function (request) {
  this.desired = request;
  var generation = ++this.generation;
  var self = this;
  return this.serial(function () {
    if (generation !== self.generation) return;
    return self.send(request);
  });
};

Capture.prototype.warm = function (base) {
  if (!this.desired) this.desired = { method: 'warm', base: base };
  var self = this;
  return this.serial(function () { return self.send(self.desired); });
};
Capture.prototype.prepare = function (base, payload) {
  return this.prepareRequest({ method: 'prepare', base: base, payload: payload });
};
Capture.prototype.preparePage = function (payload) {
  return this.prepareRequest({ method: 'preparePage', payload: payload, inject: this.options.inject });
};
Capture.prototype.capture = function (base, payload) {
  this.desired = { method: 'prepare', base: base, payload: payload };
  ++this.generation;
  var self = this;
  return this.serial(function () { return self.send({ method: 'capture', base: base, payload: payload }); })
    .then(function (result) {
      var image = Buffer.from(result.data, 'base64');
      Object.defineProperty(image, 'captureDetails', { value: result.details });
      return image;
    });
};
Capture.prototype.capturePage = function (payload) {
  this.desired = { method: 'preparePage', payload: payload, inject: this.options.inject };
  ++this.generation;
  var request = Object.assign({}, this.desired, {
    method: 'capturePage', overlay: scripts.overlayScript(payload), removeOverlay: scripts.removeOverlayScript,
    describe: payload.anchors && payload.anchors.length ? scripts.describeScript(payload.anchors) : null,
  });
  var self = this;
  return this.serial(function () { return self.send(request); }).then(function (result) {
    var image = Buffer.from(result.data, 'base64');
    var captured = payload.format === 'jpeg' ? { image: image, targets: result.targets } : { png: image, targets: result.targets };
    if (result.details) captured.details = result.details;
    return captured;
  });
};
Capture.prototype.captureExportPage = function (payload) {
  var request = {
    method: 'captureExportPage', payload: payload, inject: this.options.inject,
    settle: scripts.settleScript(), settleFast: scripts.settleScript({ quiet: false, imageTimeout: 250 }),
    switchStory: scripts.storybookSwitchScript(payload.url),
    overlay: scripts.overlayScript(payload), removeOverlay: scripts.removeOverlayScript,
  };
  this.desired = request;
  ++this.generation;
  var self = this;
  return this.serial(function () { return self.send(request); }).then(function (result) {
    var image = Buffer.from(result.data, 'base64');
    var captured = payload.format === 'jpeg' ? { image: image } : { png: image };
    if (result.details) captured.details = result.details;
    return captured;
  });
};
Capture.prototype.close = function () {
  if (this.closing) return this.closing;
  this.closed = true;
  clearTimeout(this.retry);
  var child = this.child;
  if (!child) return Promise.resolve();
  this.closing = new Promise(function (resolve) {
    var timer = setTimeout(function () { child.kill('SIGKILL'); }, 1500);
    child.once('exit', function () { clearTimeout(timer); resolve(); });
    child.stdin.end(JSON.stringify({ method: 'close' }) + '\n');
  });
  return this.closing;
};

Capture.prototype.printExportPage = function (payload) {
  var request = { method: 'printExportPage', payload: payload, inject: this.options.inject,
    settle: scripts.settleScript(), settleFast: scripts.settleScript({ quiet: false, imageTimeout: 250 }),
    switchStory: scripts.storybookSwitchScript(payload.url) };
  this.desired = request;
  ++this.generation;
  var self = this;
  return this.serial(function () { return self.send(request); }).then(function (result) {
    return { pdf: Buffer.from(result.data, 'base64'), width: result.width, height: result.height };
  });
};

var METHODS = ['warm', 'prepare', 'preparePage', 'capture', 'capturePage', 'captureExportPage', 'printExportPage'];

/* The service the server uses. A helper that exits mid-request gets that
   request once more on its replacement. On a host that can't run the bundled
   runtime, every request fails with the reason instead. */
function createService(options) {
  options = options || {};
  var reason = options.spawn || options.executable ? null : runtime.unavailable();
  var helper = reason ? null : new Capture(options);
  var closed = false;
  var service = {};
  METHODS.forEach(function (method) {
    service[method] = async function () {
      if (closed) throw new Error('Capture service is closed');
      if (reason) throw new Error('Screenshots need the bundled capture runtime, and ' + reason + '.');
      try { return await helper[method].apply(helper, arguments); }
      catch (error) {
        if (error.code !== 'CAPTURE_PROCESS_EXITED' || closed) throw error;
        return helper[method].apply(helper, arguments);
      }
    };
  });
  service.close = function () {
    closed = true;
    return helper ? helper.close() : Promise.resolve();
  };
  service.createPool = function (size) {
    if (closed) throw new Error('Capture service is closed');
    var extras = Array.from({ length: Math.max(0, (Math.floor(size) || 1) - 1) }, function () {
      return createService(options);
    });
    return {
      // Activation has already warmed this service. Let the export make
      // progress on it while the remaining parallel workers cold-start.
      workers: [service].concat(extras),
      close: function () { return Promise.all(extras.map(function (worker) { return worker.close(); })); },
    };
  };
  return service;
}

module.exports = { create: function (options) { return new Capture(options); }, createService: createService, Capture: Capture };
