/* Bring up implementations that explicitly declare a local start command.
   The check runs before the terminal is opened and again until the service
   answers. This keeps extension reloads from starting duplicate processes. */

var net = require('net');
var path = require('path');
var config = require('./config');

function portOpen(host, port) {
  return new Promise(function (resolve) {
    var socket = net.connect({ host: host, port: port });
    var settled = false;
    function finish(open) {
      if (settled) return;
      settled = true;
      socket.destroy();
      resolve(open);
    }
    socket.setTimeout(1000, function () { finish(false); });
    socket.once('connect', function () { finish(true); });
    socket.once('error', function () { finish(false); });
  });
}

function check(spec) {
  if (spec.port) return portOpen(spec.host, spec.port);
  return fetch(spec.url, { signal: AbortSignal.timeout(1500) }).then(function (response) {
    return response.ok;
  }).catch(function () { return false; });
}

function wait(ms) {
  return new Promise(function (resolve) { setTimeout(resolve, ms); });
}

async function startOne(root, impl, options) {
  var start = impl.start;
  var probe = options.probe || check;
  var report = options.report || function () {};
  if (await probe(start.check)) {
    report(impl.key + ': already running');
  } else {
    var terminal = options.createTerminal({
      name: 'Workbench: ' + impl.label,
      cwd: path.resolve(root, start.cwd),
    });
    terminal.show(true);
    terminal.sendText(start.command, true);
    report(impl.key + ': started ' + start.command);
  }

  var readiness = start.ready || start.check;
  if (await probe(readiness)) {
    report(impl.key + ': ready');
    return;
  }
  var deadline = Date.now() + start.timeout * 1000;
  do {
    await (options.wait || wait)(500);
    if (await probe(readiness)) {
      report(impl.key + ': ready');
      return;
    }
  } while (Date.now() < deadline);
  report(impl.key + ': did not become ready within ' + start.timeout + ' seconds');
}

/* `where` is the project's folder, or { dir, key } for one project of a
   workbench.yaml that lists several; see config.read. */
function run(where, options) {
  options = options || {};
  var read = config.read(where);
  var root = read ? read.root : null;
  if (!read) return Promise.resolve();
  var starts = Object.keys(read.implementations).map(function (key) {
    return read.implementations[key];
  }).filter(function (impl) { return !!impl.start; });
  if (!starts.length) return Promise.resolve();
  if (!options.isTrusted) {
    (options.report || function () {})('Workspace is not trusted; implementation start commands were skipped.');
    return Promise.resolve();
  }
  return Promise.all(starts.map(function (impl) {
    return startOne(root, impl, options).catch(function (error) {
      (options.report || function () {})(impl.key + ': could not start — ' + String(error.message || error));
    });
  })).then(function () {});
}

module.exports = { run: run, check: check };
