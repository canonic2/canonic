var assert = require('node:assert/strict');
var http = require('node:http');
var test = require('node:test');

var remote = require('./remote');

function serve(handler) {
  return new Promise(function (resolve) {
    var server = http.createServer(handler);
    server.listen(0, '127.0.0.1', function () {
      resolve({
        url: 'http://127.0.0.1:' + server.address().port,
        close: function () {
          return new Promise(function (done) {
            server.close(done);
          });
        },
      });
    });
  });
}

test('fetches JSON, and says when it is not', async function () {
  var stub = await serve(function (req, res) {
    if (req.url === '/index.json') {
      res.writeHead(200, { 'Content-Type': 'application/json' });
      res.end(JSON.stringify({ v: 5, entries: {} }));
      return;
    }
    res.writeHead(200, { 'Content-Type': 'text/html' });
    res.end('<html>');
  });
  try {
    var index = await remote.fetchJson(stub.url + '/index.json');
    assert.equal(index.status, 200);
    assert.deepEqual(index.body, { v: 5, entries: {} });

    var page = await remote.fetchJson(stub.url + '/');
    assert.equal(page.status, 200);
    assert.equal(page.body, undefined);
  } finally {
    await stub.close();
  }
});

test('reports a port nobody answers on', async function () {
  var stub = await serve(function () {});
  var url = stub.url;
  await stub.close();
  await assert.rejects(remote.fetchJson(url + '/index.json', { timeout: 500 }), /ECONNREFUSED/);
});
