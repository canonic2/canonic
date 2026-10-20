/* The implementation proxies, in the Workbench server: a URL lens is framed
   through one, everything passes through, documents gain the bridge, and
   nothing the implementation sends keeps it out of the frame. */

var test = require('node:test');
var assert = require('node:assert');
var fs = require('node:fs');
var http = require('node:http');
var net = require('node:net');
var os = require('node:os');
var path = require('node:path');

var server = require('./server');

function getJson(port, route) {
  return new Promise(function (resolve, reject) {
    http.get({ host: '127.0.0.1', port: port, path: route, agent: false }, function (res) {
      var body = '';
      res.on('data', function (chunk) { body += chunk; });
      res.on('end', function () { resolve(JSON.parse(body)); });
    }).on('error', reject);
  });
}

/* A space whose URL lens is `implementation`, as the canvas reads it: the
   config answers with the implementation at its proxy. */
async function proxyClient(implementation) {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'acme-proxy-'));
  fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Acme\nimplementations:\n  dev:\n    label: Dev\n    kind: url\n    base: ' + implementation + '\n');
  var running = await server.start({ root: root, capture: { close: function () {} } });
  async function proxied() {
    var config = await getJson(running.port, '/_workbench/config');
    return config.implementations.dev;
  }
  var first = await proxied();
  return {
    tag: '<script src="http://127.0.0.1:' + running.port + '/_workbench/preview-bridge.js"></script>',
    origin: new URL(first.base).origin,
    upstream: first.upstream,
    proxied: proxied,
    close: function () { return running.close().then(function () { fs.rmSync(root, { recursive: true, force: true }); }); },
  };
}

function upstream() {
  var seen = [];
  var server = http.createServer(function (req, res) {
    seen.push({ url: req.url, headers: req.headers });
    if (req.url.split('?')[0] === '/iframe.html') {
      res.writeHead(200, {
        'Content-Type': 'text/html; charset=utf-8',
        'Content-Security-Policy': "frame-ancestors 'none'",
        'X-Frame-Options': 'DENY',
        'ETag': '"v1"',
        'Set-Cookie': 'session=acme; Domain=example.com; Path=/; Secure; SameSite=None; HttpOnly',
      });
      res.end('<!doctype html><html><head><title>Story</title></head><body>Acme</body></html>');
      return;
    }
    if (req.url === '/fragment') {
      res.writeHead(200, { 'Content-Type': 'text/html' });
      res.end('<p>data</p>');
      return;
    }
    if (req.url === '/moved') {
      res.writeHead(302, { Location: 'http://' + req.headers.host + '/iframe.html' });
      res.end();
      return;
    }
    if (req.method === 'POST') {
      var body = '';
      req.on('data', function (chunk) { body += chunk; });
      req.on('end', function () { res.writeHead(201, { 'Content-Type': 'text/plain' }); res.end('got ' + body); });
      return;
    }
    res.writeHead(200, { 'Content-Type': 'text/javascript' });
    res.end('export default 1;');
  });
  /* A minimal upgrade echo: answer 101, then send back what arrives. */
  server.on('upgrade', function (req, socket) {
    seen.push({ url: req.url, headers: req.headers, upgrade: true });
    socket.write('HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n');
    socket.on('data', function (chunk) { socket.write(chunk); });
  });
  return new Promise(function (resolve) {
    server.listen(0, '127.0.0.1', function () {
      resolve({ server: server, seen: seen, origin: 'http://127.0.0.1:' + server.address().port });
    });
  });
}

function request(address, options) {
  options = options || {};
  return new Promise(function (resolve, reject) {
    var req = http.request(address, { method: options.method || 'GET', headers: options.headers || {} }, function (res) {
      var body = '';
      res.setEncoding('utf8');
      res.on('data', function (chunk) { body += chunk; });
      res.on('end', function () { resolve({ status: res.statusCode, headers: res.headers, body: body }); });
    });
    req.on('error', reject);
    req.end(options.body);
  });
}

async function setup() {
  var up = await upstream();
  var proxies = await proxyClient(up.origin);
  var origin = proxies.origin;
  var address = origin + '/iframe.html?id=button--primary';
  return {
    up: up, proxies: proxies, address: address, origin: origin, tag: proxies.tag,
    close: function () { return proxies.close().then(function () { up.server.closeAllConnections(); up.server.close(); }); },
  };
}

test('a URL lens is framed through its proxy, one per origin, keeping its own address', async function () {
  var made = await setup();
  try {
    assert.match(made.origin, /^http:\/\/127\.0\.0\.1:\d+$/);
    assert.notStrictEqual(made.origin, made.up.origin);
    assert.strictEqual(made.proxies.upstream, made.up.origin);
    assert.strictEqual(new URL((await made.proxies.proxied()).base).origin, made.origin);
  } finally { await made.close(); }
});

test('puts the bridge first in a framed document and lifts frame blockers', async function () {
  var made = await setup();
  try {
    var answer = await request(made.address, { headers: { 'Sec-Fetch-Dest': 'iframe', 'Accept-Encoding': 'gzip' } });
    assert.strictEqual(answer.status, 200);
    assert.ok(answer.body.indexOf('<head>' + made.tag + '<title>') !== -1, answer.body);
    assert.strictEqual(Number(answer.headers['content-length']), Buffer.byteLength(answer.body));
    assert.strictEqual(answer.headers['content-security-policy'], undefined);
    assert.strictEqual(answer.headers['x-frame-options'], undefined);
    assert.strictEqual(answer.headers.etag, undefined);
    assert.strictEqual(answer.headers['cache-control'], 'no-store');
    assert.deepStrictEqual(answer.headers['set-cookie'], ['session=acme; Path=/; HttpOnly']);
    /* The implementation sees itself addressed, uncompressed. */
    var seen = made.up.seen[0];
    assert.strictEqual(seen.headers.host, new URL(made.up.origin).host);
    assert.strictEqual(seen.headers['accept-encoding'], undefined);
  } finally { await made.close(); }
});

test('leaves scripts, fetched HTML and request bodies alone', async function () {
  var made = await setup();
  try {
    var script = await request(made.origin + '/@vite/client');
    assert.strictEqual(script.body, 'export default 1;');
    var fragment = await request(made.origin + '/fragment', { headers: { 'Sec-Fetch-Dest': 'empty' } });
    assert.strictEqual(fragment.body, '<p>data</p>');
    var posted = await request(made.origin + '/save', {
      method: 'POST', body: 'acme', headers: { Origin: made.origin, 'Content-Type': 'text/plain' },
    });
    assert.strictEqual(posted.status, 201);
    assert.strictEqual(posted.body, 'got acme');
    assert.strictEqual(made.up.seen[made.up.seen.length - 1].headers.origin, made.up.origin);
  } finally { await made.close(); }
});

test('keeps redirects on the proxy', async function () {
  var made = await setup();
  try {
    var answer = await request(made.origin + '/moved');
    assert.strictEqual(answer.status, 302);
    assert.strictEqual(answer.headers.location, made.origin + '/iframe.html');
  } finally { await made.close(); }
});

test('passes WebSocket upgrades through both ways', async function () {
  var made = await setup();
  try {
    var port = Number(new URL(made.origin).port);
    var echoed = await new Promise(function (resolve, reject) {
      var socket = net.connect(port, '127.0.0.1', function () {
        socket.write('GET /storybook-server-channel HTTP/1.1\r\nHost: 127.0.0.1:' + port +
          '\r\nOrigin: ' + made.origin + '\r\nUpgrade: websocket\r\nConnection: Upgrade\r\n\r\n');
      });
      var received = '';
      socket.on('data', function (chunk) {
        received += chunk;
        if (/\r\n\r\n$/.test(received) && received.indexOf('ping') === -1) socket.write('ping');
        if (received.indexOf('ping') !== -1) { socket.destroy(); resolve(received); }
      });
      socket.on('error', reject);
    });
    assert.match(echoed, /^HTTP\/1\.1 101/);
    var upgrade = made.up.seen.find(function (entry) { return entry.upgrade; });
    assert.strictEqual(upgrade.url, '/storybook-server-channel');
    assert.strictEqual(upgrade.headers.host, new URL(made.up.origin).host);
    assert.strictEqual(upgrade.headers.origin, made.up.origin);
  } finally { await made.close(); }
});

test('answers 502 when the implementation is not running', async function () {
  var closed = net.createServer();
  await new Promise(function (resolve) { closed.listen(0, '127.0.0.1', resolve); });
  var dead = 'http://127.0.0.1:' + closed.address().port;
  await new Promise(function (resolve) { closed.close(resolve); });
  var proxies = await proxyClient(dead);
  try {
    var answer = await request(proxies.origin + '/');
    assert.strictEqual(answer.status, 502);
    assert.match(answer.body, /isn’t answering/);
  } finally { await proxies.close(); }
});
