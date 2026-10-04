'use strict';

var test = require('node:test');
var assert = require('node:assert');
var fs = require('node:fs');
var os = require('node:os');
var path = require('node:path');

var agentView = require('./agent-view');

var SIGN_IN = { text: '- Screen: Sign in — `pages/sign-in.html`\n- State: Error — `error`\n- Lens: Design',
  src: 'pages/sign-in.html', state: 'error' };
var HOME = { text: '- Screen: Home — `pages/home.html`\n- State: Default — `default`\n- Lens: Design',
  src: 'pages/home.html' };

function clock(start) {
  var at = start;
  return { now: function () { return at; }, tick: function (ms) { at += ms; } };
}

test('reports no view until a canvas says what it shows', function () {
  var root = path.resolve(os.tmpdir());
  var shown = agentView.create({ root: root });
  assert.deepEqual(shown.current(), { root: root, open: false, view: null, changedAt: null });
  shown.report({ client: 'tab', view: null });
  assert.deepEqual(shown.current(), { root: root, open: true, view: null, changedAt: shown.current().changedAt });
});

test('follows the canvas changed most recently, and keeps its change time across heartbeats', function () {
  var time = clock(Date.UTC(2026, 9, 3, 12));
  var shown = agentView.create({ root: os.tmpdir(), now: time.now });
  shown.report({ client: 'tab', view: SIGN_IN });
  time.tick(1000);
  shown.report({ client: 'browser', view: HOME });
  assert.equal(shown.current().view.src, 'pages/home.html');

  time.tick(1000);
  shown.report({ client: 'tab', view: SIGN_IN });
  assert.equal(shown.current().view.src, 'pages/home.html', 'a heartbeat is not a change');

  time.tick(1000);
  shown.report({ client: 'tab', view: Object.assign({}, SIGN_IN, { state: 'locked' }) });
  assert.equal(shown.current().view.state, 'locked');
  assert.equal(shown.current().changedAt, new Date(Date.UTC(2026, 9, 3, 12) + 3000).toISOString());
});

test('drops a canvas that closes or stops reporting', function () {
  var time = clock(0);
  var shown = agentView.create({ root: os.tmpdir(), now: time.now, staleMs: 1000 });
  shown.report({ client: 'tab', view: SIGN_IN });
  shown.report({ client: 'browser', view: HOME });
  shown.report({ client: 'browser', closed: true });
  assert.equal(shown.current().view.src, 'pages/sign-in.html');
  time.tick(1001);
  assert.equal(shown.current().open, false);
});

test('keeps only the reference and its ids', function () {
  var shown = agentView.create({ root: os.tmpdir() });
  shown.report({ client: 'tab', view: Object.assign({ html: '<p>secret</p>', state: 5 }, HOME) });
  assert.deepEqual(shown.current().view, HOME);
  assert.throws(function () { shown.report({ view: HOME }); }, /client id/);
});

test('announces its address only in a Canonic project, and removes only its own file', function (t) {
  var plain = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-view-plain-'));
  var project = fs.mkdtempSync(path.join(os.tmpdir(), 'canonic-view-project-'));
  t.after(function () {
    fs.rmSync(plain, { recursive: true, force: true });
    fs.rmSync(project, { recursive: true, force: true });
  });
  fs.mkdirSync(path.join(project, '.canonic'));
  var file = path.join(project, agentView.ANNOUNCE_FILE);

  var outside = agentView.create({ root: plain });
  outside.announce('http://127.0.0.1:3579/');
  assert.equal(fs.existsSync(path.join(plain, '.canonic')), false);

  var first = agentView.create({ root: project });
  first.announce('http://127.0.0.1:3579/');
  assert.equal(JSON.parse(fs.readFileSync(file, 'utf8')).url, 'http://127.0.0.1:3579/');
  var second = agentView.create({ root: project });
  second.announce('http://127.0.0.1:3580/');
  first.close();
  assert.equal(JSON.parse(fs.readFileSync(file, 'utf8')).url, 'http://127.0.0.1:3580/');
  second.close();
  assert.equal(fs.existsSync(file), false);
});
