var assert = require('node:assert/strict');
var test = require('node:test');
var EventEmitter = require('node:events');
var PassThrough = require('node:stream').PassThrough;
var simulatorStream = require('./simulator-stream');

function child() {
  var proc = new EventEmitter();
  proc.stdout = new PassThrough();
  proc.stderr = new PassThrough();
  proc.killed = false;
  proc.kill = function () {
    if (proc.killed) return;
    proc.killed = true;
    setImmediate(function () { proc.emit('exit', 0, 'SIGTERM'); });
  };
  return proc;
}

test('starts one native stream and sends clients complete H.264 packets', async function (t) {
  var proc = child();
  var starts = [];
  var manager = simulatorStream.create({
    prepare: async function () { return '/helper'; },
    permissionOwner: 'Visual Studio Code',
    spawn: function (file, args, options) { starts.push([file, args, options]); return proc; },
  });
  t.after(function () { return manager.close(); });
  var started = manager.start({ udid: 'SIM-1', source: 'iPhone 17' });
  proc.stderr.write('Streaming iPhone 17 at 480x1092, 30 fps\n');
  var answer = await started;
  assert.equal(starts[0][0], '/helper');
  assert.deepEqual(starts[0][1], ['iPhone 17']);
  assert.equal(starts[0][2].env.CANONIC_SCREEN_CAPTURE_OWNER, 'Visual Studio Code');
  assert.equal(answer.source, 'iPhone 17');
  assert.equal(answer.codec, 'h264');
  assert.match(answer.token, /^[a-f0-9]{48}$/);

  var writes = [];
  var socket = new EventEmitter();
  socket.destroyed = false;
  socket.write = function (data) { writes.push(data); return true; };
  socket.destroy = function () { socket.destroyed = true; socket.emit('close'); };
  assert.equal(manager.accept({
    url: '/_workbench/simulator/stream?token=' + answer.token,
    headers: { 'sec-websocket-key': 'test-key' },
  }, socket), true);

  var encoded = Buffer.from([0, 0, 0, 1, 0x65, 1, 2, 3]);
  var packet = Buffer.alloc(13 + encoded.length);
  packet.writeUInt32BE(encoded.length, 0);
  packet.writeBigUInt64BE(1234n, 4);
  packet[12] = 1;
  encoded.copy(packet, 13);
  proc.stdout.write(packet.subarray(0, 9));
  proc.stdout.write(packet.subarray(9));
  assert.equal(writes.length, 2);
  assert.match(writes[0].toString(), /101 Switching Protocols/);
  assert.equal(writes[1][0], 0x82);
  assert.equal(writes[1].subarray(-encoded.length).compare(encoded), 0);
});

test('starts a JPEG stream for an embedded webview client', async function (t) {
  var proc = child();
  var starts = [];
  var manager = simulatorStream.create({
    prepare: async function () { return '/helper'; },
    spawn: function (file, args) { starts.push([file, args]); return proc; },
  });
  t.after(function () { return manager.close(); });
  var started = manager.start({ udid: 'SIM-1', source: 'iPhone 17', codec: 'jpeg' });
  proc.stderr.write('Streaming iPhone 17 as JPEG at 480x1092, 20 fps\n');
  var answer = await started;
  assert.deepEqual(starts[0], ['/helper', ['--jpeg', 'iPhone 17']]);
  assert.equal(answer.codec, 'jpeg');
});

test('rejects a WebSocket without the active private token', function () {
  var manager = simulatorStream.create();
  assert.equal(manager.accept({ url: '/_workbench/simulator/stream?token=nope', headers: {} }, {}), false);
});

test('streams framed packets over HTTP for an embedded webview', async function (t) {
  var proc = child();
  var manager = simulatorStream.create({
    prepare: async function () { return '/helper'; },
    spawn: function () { return proc; },
  });
  t.after(function () { return manager.close(); });
  var started = manager.start({ udid: 'SIM-1', source: 'iPhone 17', codec: 'jpeg' });
  proc.stderr.write('Streaming iPhone 17 as JPEG at 480x1092, 20 fps\n');
  var answer = await started;
  var response = new EventEmitter();
  var request = new EventEmitter();
  var writes = [];
  response.destroyed = false;
  response.writeHead = function (status, headers) { response.status = status; response.headers = headers; };
  response.flushHeaders = function () {};
  response.write = function (data) { writes.push(data); return true; };
  response.end = function () { response.ended = true; };
  assert.equal(manager.acceptHttp({
    url: '/_workbench/simulator/stream?token=' + answer.token,
    on: request.on.bind(request),
  }, response), true);
  assert.equal(response.status, 200);

  var jpeg = Buffer.from([0xff, 0xd8, 0xff, 0xd9]);
  var framed = Buffer.alloc(13 + jpeg.length);
  framed.writeUInt32BE(jpeg.length, 0);
  framed.writeBigUInt64BE(42n, 4);
  framed[12] = 1;
  jpeg.copy(framed, 13);
  proc.stdout.write(framed);
  assert.equal(writes.length, 1);
  assert.equal(writes[0].readUInt32BE(0), writes[0].length - 4);
  assert.equal(writes[0].subarray(-jpeg.length).compare(jpeg), 0);

  var late = new EventEmitter();
  var lateRequest = new EventEmitter();
  var lateWrites = [];
  late.destroyed = false;
  late.writeHead = function () {};
  late.flushHeaders = function () {};
  late.write = function (data) { lateWrites.push(data); return true; };
  late.end = function () {};
  assert.equal(manager.acceptHttp({
    url: '/_workbench/simulator/stream?token=' + answer.token,
    on: lateRequest.on.bind(lateRequest),
  }, late), true);
  assert.equal(lateWrites.length, 1, 'a late client immediately receives the retained frame');
  assert.equal(lateWrites[0].subarray(-jpeg.length).compare(jpeg), 0);
});

test('does not repeat the helper error prefix in the workbench message', function () {
  assert.equal(
    simulatorStream.helperError(
      'Requesting Screen Recording permission for Visual Studio Code…\n' +
        'Simulator stream failed: Enable Visual Studio Code in Screen & System Audio Recording, restart it, then retry.\n',
      'stopped'
    ),
    'Enable Visual Studio Code in Screen & System Audio Recording, restart it, then retry.'
  );
});
