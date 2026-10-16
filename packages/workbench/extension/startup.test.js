var assert = require('node:assert/strict');
var fs = require('node:fs');
var net = require('node:net');
var os = require('node:os');
var path = require('node:path');
var test = require('node:test');
var startup = require('./startup');

function space() {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-startup-'));
  fs.writeFileSync(path.join(root, 'workbench.yaml'), [
    'implementations:',
    '  storybook:',
    '    kind: storybook',
    '    url: http://localhost:6006',
    '    start:',
    '      command: yarn storybook',
    '      cwd: packages/storybook',
    '      check:',
    '        port: 6006',
    '      ready:',
    '        url: http://localhost:6006/index.json',
    '      timeout: 5',
  ].join('\n'));
  return root;
}

test('starts the configured command only when its readiness check fails', async function () {
  var root = space();
  var terminals = [];
  var probes = 0;
  try {
    var options = {
      isTrusted: true,
      probe: function () { probes += 1; return Promise.resolve(probes > 1); },
      wait: function () { return Promise.resolve(); },
      createTerminal: function (spec) {
        var terminal = { spec: spec, show: function (preserve) { this.preserve = preserve; },
          sendText: function (command, execute) { this.command = command; this.execute = execute; } };
        terminals.push(terminal);
        return terminal;
      },
    };
    await startup.run(root, options);
    assert.equal(terminals.length, 1);
    assert.equal(terminals[0].spec.cwd, path.join(root, 'packages/storybook'));
    assert.equal(terminals[0].command, 'yarn storybook');
    assert.equal(terminals[0].execute, true);
    assert.equal(terminals[0].preserve, true);

    options.probe = function () { return Promise.resolve(true); };
    await startup.run(root, options);
    assert.equal(terminals.length, 1);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('never runs a configured command in an untrusted workspace', async function () {
  var root = space();
  try {
    await startup.run(root, {
      isTrusted: false,
      createTerminal: function () { assert.fail('terminal must not be created'); },
    });
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('waits for the readiness URL when the port is already open', async function () {
  var root = space();
  var checks = [];
  try {
    await startup.run(root, {
      isTrusted: true,
      probe: function (spec) {
        checks.push(spec);
        return Promise.resolve(spec.port ? true : checks.length >= 3);
      },
      wait: function () { return Promise.resolve(); },
      createTerminal: function () { assert.fail('an open port must not launch a second process'); },
    });
    assert.deepEqual(checks.map(function (spec) { return spec.port ? 'port' : 'url'; }),
      ['port', 'url', 'url']);
  } finally {
    fs.rmSync(root, { recursive: true, force: true });
  }
});

test('the TCP readiness check detects a listening port', async function () {
  var server = net.createServer();
  await new Promise(function (resolve) { server.listen(0, '127.0.0.1', resolve); });
  var port = server.address().port;
  try {
    assert.equal(await startup.check({ host: '127.0.0.1', port: port }), true);
  } finally {
    await new Promise(function (resolve) { server.close(resolve); });
  }
  assert.equal(await startup.check({ host: '127.0.0.1', port: port }), false);
});
