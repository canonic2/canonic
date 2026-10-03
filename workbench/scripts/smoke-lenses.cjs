/* Native iframe/session check against temporary app and Storybook fixtures.
   Run explicitly with node scripts/smoke-lenses.cjs; ordinary tests need no
   installed browser. Chrome here drives the test, not the workbench's lenses. */
var assert = require('node:assert/strict');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');
var http = require('node:http');
var server = require('../server');
var Browser = require('./chrome.cjs').Browser;

async function main() {
  var root = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-lenses-smoke-'));
  var browser = new Browser();
  var running;
  var requests = [];
  var submitted = '';
  var app = http.createServer(function (req, res) {
    requests.push(req.url);
    if (req.url === '/index.json') {
      res.setHeader('Content-Type', 'application/json');
      res.end(JSON.stringify({ entries: { 'fixture--default': {
        id: 'fixture--default', type: 'story', title: 'Fixture', name: 'Default',
      } } }));
      return;
    }
    if (req.url === '/login' && req.method === 'POST') {
      req.on('data', function (chunk) { submitted += chunk; });
      req.on('end', function () {
        res.writeHead(303, { 'Set-Cookie': 'fixture-session=yes; HttpOnly; SameSite=Lax; Path=/', Location: '/app' });
        res.end();
      });
      return;
    }
    var signedIn = /fixture-session=yes/.test(req.headers.cookie || '');
    res.setHeader('Content-Type', 'text/html');
    res.end('<!doctype html><html><body style="margin:24px;font:18px sans-serif">' +
      '<h1>' + (signedIn ? 'Signed in' : 'Sign in') + '</h1>' +
      '<form method="post" action="/login"><input name="name" placeholder="Name"><button>Sign in</button></form>' +
      '<p>Selectable implementation text</p><div style="height:1500px"></div>' +
      '<script>function report(){var b=document.querySelector("button").getBoundingClientRect();' +
      'var i=document.querySelector("input").getBoundingClientRect();' +
      'parent.postMessage({type:"fixture-ready",path:location.pathname,signedIn:' + signedIn +
      ',width:innerWidth,input:{x:i.x+i.width/2,y:i.y+i.height/2},button:{x:b.x+b.width/2,y:b.y+b.height/2}},"*");}' +
      'addEventListener("load",report);addEventListener("resize",report);</script></body></html>');
  });
  await new Promise(function (resolve) { app.listen(0, '127.0.0.1', resolve); });
  var origin = 'http://127.0.0.1:' + app.address().port;
  fs.writeFileSync(path.join(root, 'fixture.html'), '<html><body><h1>Design fixture</h1></body></html>');
  fs.writeFileSync(path.join(root, 'workbench.yaml'), 'name: Iframe smoke\nimplementations:\n' +
    '  dev:\n    kind: url\n    base: ' + origin + '\n    render: browser\n' +
    '  storybook:\n    kind: storybook\n    url: ' + origin + '\n' +
    'sections:\n  - name: Screens\n    items:\n      - label: Fixture\n        src: fixture.html\n' +
    '        implementations:\n          dev: /app\n          storybook: Fixture\n');
  try {
    // Native captures are exercised separately by smoke-capture.cjs.
    running = await server.start({ root: root, capture: {
      warm: function () {}, prepare: function () {}, preparePage: function () {}, close: function () {},
    } });
    await browser.launch();
    var target = await browser.createTarget();
    var failures = [];
    var traffic = [];
    target.on('Runtime.exceptionThrown', function (event) { failures.push(event.exceptionDetails.text); });
    target.on('Network.requestWillBeSent', function (event) { traffic.push(event.request.url); });
    await target.send('Network.enable');
    await target.send('Emulation.setDeviceMetricsOverride', {
      width: 2000, height: 1000, deviceScaleFactor: 1, mobile: false,
    });
    await target.send('Page.addScriptToEvaluateOnNewDocument', { source:
      'window.fixtureMessages=[];addEventListener("message",function(e){' +
      'if(e.origin===' + JSON.stringify(origin) + '&&e.data.type==="fixture-ready")window.fixtureMessages.push(e.data);});',
    });
    async function until(expression) {
      var deadline = Date.now() + 10000;
      while (Date.now() < deadline) {
        if (await target.evaluate(expression)) return;
        await new Promise(function (resolve) { setTimeout(resolve, 50); });
      }
      throw new Error('Iframe did not settle: ' + expression);
    }
    async function pick(hash, pathname) {
      await target.evaluate('window.fixtureMessages=[];location.hash=' + JSON.stringify(hash));
      await until('window.fixtureMessages.some(function(m){return m.path===' + JSON.stringify(pathname) + ';})');
      await until('!document.getElementById("frameShell").hidden && document.querySelector("iframe.is-active").src.includes(' + JSON.stringify(pathname) + ')');
    }
    await target.navigate(running.url + '#fixture.html@393~dev');
    await until('window.fixtureMessages.length && !document.getElementById("frameShell").hidden');
    assert.equal(await target.evaluate('window.fixtureMessages.at(-1).width'), 393);
    assert.equal(await target.evaluate('window.fixtureMessages.at(-1).signedIn'), false);
    // Click the actual iframe form: no input forwarding or fake login API.
    async function click(field) {
      var point = await target.evaluate('(function(){var f=document.querySelector("iframe.is-active").getBoundingClientRect();' +
        'var b=window.fixtureMessages.at(-1)[' + JSON.stringify(field) + '];return {x:f.x+b.x,y:f.y+b.y};})()');
      await target.send('Input.dispatchMouseEvent', { type: 'mousePressed', x: point.x, y: point.y, button: 'left', clickCount: 1 });
      await target.send('Input.dispatchMouseEvent', { type: 'mouseReleased', x: point.x, y: point.y, button: 'left', clickCount: 1 });
    }
    await click('input');
    await target.send('Input.insertText', { text: 'Acme' });
    await click('button');
    await until('window.fixtureMessages.at(-1).signedIn');
    assert.equal(new URLSearchParams(submitted).get('name'), 'Acme');
    await target.evaluate('window.fixtureMessages=[];document.getElementById("reload").click()');
    await until('window.fixtureMessages.length && window.fixtureMessages.at(-1).signedIn');
    await pick('#fixture.html@393~storybook', '/iframe.html');
    await pick('#fixture.html@393~dev', '/app');
    assert.equal(await target.evaluate('window.fixtureMessages.at(-1).signedIn'), true);
    await pick('#fixture.html@1512~dev', '/app');
    assert.equal(await target.evaluate('window.fixtureMessages.at(-1).width'), 1512);
    await target.evaluate('document.querySelector("#lenses button").click()');
    await until('document.querySelector("iframe.is-active").src.includes("/fixture.html")');
    await pick('#fixture.html@393~dev', '/app');
    assert.equal(await target.evaluate('window.fixtureMessages.at(-1).signedIn'), true);
    assert.ok(!traffic.some(function (url) { return /\/_workbench\/(live\/|lens\/probe)/.test(url); }));
    assert.deepEqual(failures, []);
    fs.writeFileSync(path.join(root, 'iframe-session.png'), await target.screenshot());
    console.log(JSON.stringify({ root: root, workbench: running.url, sessionSurvived: true, requests: requests }, null, 2));
  } finally {
    await browser.close();
    if (running) await running.close();
    await new Promise(function (resolve) { app.close(resolve); });
  }
}
main().catch(function (error) { console.error(error); process.exitCode = 1; });
