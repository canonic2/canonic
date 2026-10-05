/* The worker owns compilation and its loopback server. A packaged extension
   uses the bundled Electron Node runtime; standalone use needs only Node. */
var cp = require('node:child_process');
var path = require('node:path');
var runtime = require('./electron-runtime');
var remote = require('./remote');

function create(root, options) {
  options = options || {};
  var child = null;
  var starting = null;
  var address = null;
  var closed = false;
  function diagnostic(level, event, details) {
    if (!options.diagnostic) return;
    try { options.diagnostic(level, event, details); } catch (_) {}
  }
  function launch() {
    if (closed) return Promise.reject(new Error('Preview service is closed'));
    if (starting) return starting;
    var began = Date.now();
    var bundled = runtime.available();
    diagnostic('info', 'preview.worker.starting', { runtime: bundled ? 'electron' : 'node' });
    starting = Promise.resolve().then(function () {
      return bundled ? runtime.prepare(options.storage, null, function (event, details) {
        diagnostic('info', event, Object.assign({ for: 'preview' }, details));
      }) : process.execPath;
    }).then(function (executable) {
      var spawned = Date.now();
      return new Promise(function (resolve, reject) {
        var env = Object.assign({}, process.env, { ELECTRON_RUN_AS_NODE: '1' });
        delete env.NODE_OPTIONS;
        var worker = cp.fork(path.join(__dirname, 'preview', 'worker.cjs'), [root, JSON.stringify(options.previews || {})], {
          execPath: executable, execArgv: [], env: env, stdio: ['ignore', 'ignore', 'pipe', 'ipc'],
        });
        child = worker;
        var timer = setTimeout(function () { worker.kill(); reject(new Error('Preview worker startup timed out')); }, 30000);
        worker.stderr.on('data', function (chunk) { if (options.log) options.log(String(chunk).slice(0, 2000)); });
        worker.on('message', function (message) {
          if (message.type === 'diagnostic' && options.diagnostic) {
            try { options.diagnostic('info', message.event, message.details); } catch (_) {}
          }
        });
        worker.once('message', function (message) {
          clearTimeout(timer);
          /* `launchMs` is the process alone: on macOS a newly unpacked
             runtime's first launch includes the system's code check. */
          diagnostic('info', 'preview.worker.ready', {
            prepareMs: spawned - began, launchMs: Date.now() - spawned, elapsedMs: Date.now() - began,
          });
          address = 'http://127.0.0.1:' + message.port;
          resolve(address);
        });
        worker.once('error', function (error) { clearTimeout(timer); reject(error); });
        worker.once('exit', function () {
          clearTimeout(timer);
          if (child === worker) { child = null; starting = null; address = null; }
          reject(new Error('Preview worker stopped'));
        });
      });
    }).catch(function (error) {
      diagnostic('warn', 'preview.worker.failed', { elapsedMs: Date.now() - began, message: String(error.message || error) });
      starting = null;
      throw error;
    });
    return starting;
  }
  return {
    index: function () { return launch().then(function (base) { return remote.fetchJson(base + '/index', { timeout: 120000 }); }).then(function (answer) {
      if (answer.status !== 200) throw new Error(answer.body && answer.body.error || 'Preview index failed');
      return answer.body;
    }); },
    /* A docs page lens: `index` lists its examples, `bundle` builds them. */
    docs: function (route, request) {
      return launch().then(function (base) {
        return remote.fetchJson(base + '/docs/' + route + '?request=' + encodeURIComponent(JSON.stringify(request)), { timeout: 120000 });
      }).then(function (answer) {
        if (answer.status !== 200) throw new Error(answer.body && answer.body.error || 'Docs examples failed');
        return answer.body;
      });
    },
    export: function () { return launch().then(function (base) { return remote.fetchJson(base + '/export', { timeout: 300000 }); }).then(function (answer) {
      if (answer.status !== 200) throw new Error(answer.body && answer.body.error || 'Preview export failed');
      return answer.body;
    }); },
    proxy: function (req, res, target) {
      return launch().then(function (base) {
        return new Promise(function (resolve, reject) {
          var headers = {};
          if (req.headers['if-none-match']) headers['If-None-Match'] = req.headers['if-none-match'];
          var outgoing = require('node:http').get(base + target, { headers: headers }, function (incoming) {
            res.writeHead(incoming.statusCode, incoming.headers);
            incoming.pipe(res);
            incoming.on('end', resolve);
            incoming.on('error', reject);
          });
          outgoing.setTimeout(120000, function () { outgoing.destroy(new Error('Preview request timed out')); });
          outgoing.on('error', reject);
          res.on('close', function () { outgoing.destroy(); });
        });
      });
    },
    close: function () {
      closed = true;
      return Promise.resolve(starting).catch(function () {}).then(function () {
        if (!child) return;
        return new Promise(function (resolve) {
          var worker = child;
          var timer = setTimeout(function () { worker.kill('SIGKILL'); }, 3000);
          worker.once('exit', function () { clearTimeout(timer); resolve(); });
          if (worker.connected) worker.send('close'); else worker.kill();
        });
      });
    },
  };
}
module.exports = { create: create };
