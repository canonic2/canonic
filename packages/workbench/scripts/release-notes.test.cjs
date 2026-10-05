var assert = require('node:assert/strict');
var childProcess = require('node:child_process');
var path = require('node:path');
var test = require('node:test');

var notes = require('./release-notes.cjs');

var CHANGELOG = [
  '# Changelog',
  '',
  'Intro text.',
  '',
  '## 1.2.0',
  '',
  '- Added a thing.',
  '- Fixed another.',
  '',
  '## v1.1.0',
  '',
  'A paragraph.',
  '',
  '## 1.0.0',
  '',
].join('\n');

test('answers the body of the named version, up to the next entry', function () {
  assert.equal(notes.notesFor(CHANGELOG, '1.2.0'), '- Added a thing.\n- Fixed another.\n');
  assert.equal(notes.notesFor(CHANGELOG, '1.1.0'), 'A paragraph.\n');
});

test('refuses a version without an entry, or with an empty one', function () {
  assert.throws(function () { notes.notesFor(CHANGELOG, '1.3.0'); }, /no entry for 1\.3\.0/);
  assert.throws(function () { notes.notesFor(CHANGELOG, '1.0.0'); }, /entry for 1\.0\.0 is empty/);
  assert.throws(function () { notes.notesFor(CHANGELOG, '1.2'); }, /no entry for 1\.2\b/);
});

/* Bumping the version without writing its notes fails here, before a tag. */
test('the package’s current version has notes', function () {
  var version = require('../package.json').version;
  var result = childProcess.spawnSync(process.execPath, [path.join(__dirname, 'release-notes.cjs'), version], { encoding: 'utf8' });
  assert.equal(result.status, 0, result.stderr);
  assert.ok(result.stdout.trim().length > 0);
});
