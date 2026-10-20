/* Bring up implementations that explicitly declare a local start command.
   The check runs before the terminal is opened and again until the service
   answers. This keeps extension reloads from starting duplicate processes. */

var childProcess = require('child_process');
var net = require('net');
var path = require('path');
var binaryPath = require('../server/node/binary').binaryPath;
var spaces = require('../server/node/spaces');

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

/* The space's resolved config, as the server binary reads it. */
function readConfig(space) {
  var args = ['config', space.dir].concat(space.key ? ['--space', space.key] : []);
  return new Promise(function (resolve, reject) {
    childProcess.execFile(binaryPath(), args, { encoding: 'utf8', maxBuffer: 64 * 1024 * 1024 }, function (error, out, stderr) {
      if (error) reject(new Error(String(stderr || error.message).trim()));
      else resolve(JSON.parse(out));
    });
  });
}

/* `where` is the space's folder, or { dir, key } for one space of a
   workbench.yaml that lists several, its first space without a key. */
function run(where, options) {
  options = options || {};
  var at = typeof where === 'string' ? { dir: where, key: null } : where;
  var space = spaces.listed([path.resolve(at.dir)]).filter(function (candidate) {
    return !at.key || candidate.key === at.key;
  })[0];
  if (!space) return Promise.resolve();
  return readConfig(space).then(function (read) {
    if (read) return startAll(space.root, read, options);
  });
}

function startAll(root, read, options) {
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
