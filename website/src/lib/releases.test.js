import assert from 'node:assert/strict';
import test from 'node:test';
import { TARGETS, latestDownloads, releaseDownloads, selectRelease } from './releases.js';

const repository = 'canonic2/canonic';
const targets = TARGETS.map(({ target }) => target);

function release(version, missing) {
  return {
    tag_name: `workbench/v${version}`,
    html_url: `https://github.com/${repository}/releases/tag/workbench%2Fv${version}`,
    assets: targets.filter(target => target !== missing).map(target => ({
      name: `canonic-workbench-${target}.vsix`,
      browser_download_url: `https://github.com/${repository}/releases/download/workbench%2Fv${version}/canonic-workbench-${target}.vsix`,
    })),
  };
}

test('the published website uses every asset URL from a complete release', () => {
  const downloads = releaseDownloads(release('1.0.1'), repository);
  assert.equal(downloads.version, '1.0.1');
  assert.equal(downloads.url, `https://github.com/${repository}/releases/tag/workbench%2Fv1.0.1`);
  assert.deepEqual(downloads.files.map(file => file.url), targets.map(target =>
    `https://github.com/${repository}/releases/download/workbench%2Fv1.0.1/canonic-workbench-${target}.vsix`));
});

test('the dev server links to the latest release', () => {
  const downloads = latestDownloads(repository);
  assert.equal(downloads.version, null);
  assert.ok(downloads.files.every(file => file.url.includes('/releases/latest/download/')));
});

test('the newest complete workbench release wins', () => {
  const newest = release('1.0.2');
  assert.equal(selectRelease([release('1.0.1'), release('1.0.3', 'linux-arm64'), newest]), newest);
  assert.equal(selectRelease([release('1.0.3', 'linux-arm64')]), null);
  assert.throws(() => releaseDownloads(release('1.0.3', 'linux-arm64'), repository), /missing/);
});

test('download links must stay on the repository\'s releases', () => {
  const moved = release('1.0.1');
  moved.assets[0].browser_download_url = 'https://example.com/canonic-workbench-darwin-arm64.vsix';
  assert.throws(() => releaseDownloads(moved, repository), /Unexpected download URL/);
});
