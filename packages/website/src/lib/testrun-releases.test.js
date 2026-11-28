import assert from 'node:assert/strict';
import test from 'node:test';
import { selectTestrunRelease } from './testrun-releases.js';

function release(version, properties = {}) {
  const assets = [];
  for (const arch of ['x86_64', 'aarch64']) {
    for (const [os, suffix] of [['apple-darwin', 'tar.gz'], ['pc-windows-msvc', 'zip'], ['unknown-linux-gnu', 'tar.gz']]) {
      const name = `testrun-${version}-${arch}-${os}.${suffix}`;
      assets.push(name, `${name}.sha256`);
      if (os.includes('linux')) assets.push(`${name}.sigstore.json`);
    }
  }
  return { tag_name: `testrun/v${version}`, assets: assets.map(name => ({ name, state: 'uploaded' })), ...properties };
}

test('selects the newest complete stable Testrun release without changing Workbench selection', () => {
  const complete = release('1.10.0');
  const incomplete = release('2.0.0');
  incomplete.assets.pop();
  const releases = [release('1.9.0'), incomplete, release('3.0.0', { draft: true }),
    release('4.0.0', { prerelease: true }), release('5.0.0', { tag_name: 'workbench/v5.0.0' }), complete];
  const selected = selectTestrunRelease(releases);
  assert.equal(selected.version, '1.10.0');
  assert.equal(selected.files.length, 6);
  assert.equal(selected.files.filter(file => file.signature).length, 2);
  assert.ok(selected.files.every(file => file.archive.includes('/testrun/v1.10.0/testrun-1.10.0-')));
  assert.equal(selectTestrunRelease(releases, 'testrun/v1.9.0').version, '1.9.0');
  assert.throws(() => selectTestrunRelease(releases, 'testrun/v2.0.0'), /complete platform assets/);
  assert.equal(selectTestrunRelease([incomplete]), null);
});
