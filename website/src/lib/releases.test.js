import assert from 'node:assert/strict';
import test from 'node:test';
import { TARGETS, completeReleases, latestDownloads, publishedReleases, releaseDetails, releaseDownloads, releaseNotes, selectRelease, workbenchReleases } from './releases.js';

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

test('complete releases need all six files, newest version first', () => {
  const draft = { ...release('2.0.0'), draft: true };
  const listed = completeReleases([release('1.2.0'), release('1.10.0'), draft, release('1.11.0', 'win32-x64'), release('0.9.9')]);
  assert.deepEqual(listed.map(item => item.tag_name), ['workbench/v1.10.0', 'workbench/v1.2.0', 'workbench/v0.9.9']);
});

test('the changelog lists every published release, with the files each one has', () => {
  const draft = { ...release('2.0.0'), draft: true };
  const notesOnly = { ...release('1.0.0'), assets: [] };
  const listed = publishedReleases([release('1.2.0'), notesOnly, draft, release('1.11.0', 'win32-x64')]);
  assert.deepEqual(listed.map(item => item.tag_name), ['workbench/v1.11.0', 'workbench/v1.2.0', 'workbench/v1.0.0']);
  assert.equal(releaseDetails(release('1.11.0', 'win32-x64'), repository).files.length, 5);
  assert.deepEqual(releaseDetails(notesOnly, repository).files, []);
  assert.throws(() => releaseDownloads(notesOnly, repository), /missing/);
});

test('generated release notes become text blocks and a changelog link', () => {
  const notes = releaseNotes([
    '<!-- Release notes generated using configuration in .github/release.yml -->',
    "## What's Changed",
    '* Add canvas zoom by @acme in https://github.com/canonic2/canonic/pull/4',
    '* Fix the **More** menu in [narrow windows](https://example.com/)',
    '',
    'Workbench now opens `workbench.yaml` from any folder.',
    '## New Contributors',
    '* @acme made their first contribution',
    '',
    '**Full Changelog**: https://github.com/canonic2/canonic/compare/workbench/v0.4.0...workbench/v0.5.0',
  ].join('\n'));
  assert.deepEqual(notes.blocks, [
    { items: ['Add canvas zoom by @acme in https://github.com/canonic2/canonic/pull/4', 'Fix the More menu in narrow windows'] },
    { text: 'Workbench now opens workbench.yaml from any folder.' },
  ]);
  assert.equal(notes.changes, 'https://github.com/canonic2/canonic/compare/workbench/v0.4.0...workbench/v0.5.0');
});

test('a release without its own notes links to its commit history', () => {
  const first = { ...release('1.0.1'), body: '**Full Changelog**: https://example.com/elsewhere', published_at: '2026-09-27T17:57:53Z' };
  const info = releaseDownloads(first, repository);
  assert.deepEqual(info.notes, []);
  assert.equal(info.changes, `https://github.com/${repository}/commits/workbench/v1.0.1`);
  assert.equal(info.published, '2026-09-27T17:57:53Z');
  assert.equal(info.tag, 'workbench/v1.0.1');
});

test('a build asks GitHub once per repository; uncached lookups ask every time', async t => {
  const answers = [[release('1.0.0')], [release('1.1.0')], [release('1.2.0')]];
  let calls = 0;
  t.mock.method(globalThis, 'fetch', async () => ({ ok: true, json: async () => answers[calls++] }));
  const versions = async options => (await workbenchReleases({ repository, ...options })).map(item => item.version);
  assert.deepEqual(await versions({}), ['1.0.0']);
  assert.deepEqual(await versions({}), ['1.0.0']);
  assert.deepEqual(await versions({ cache: false }), ['1.1.0']);
  assert.deepEqual(await versions({ cache: false }), ['1.2.0']);
  assert.equal(calls, 3);
});

test('download links must stay on the repository\'s releases', () => {
  const moved = release('1.0.1');
  moved.assets[0].browser_download_url = 'https://example.com/canonic-workbench-darwin-arm64.vsix';
  assert.throws(() => releaseDownloads(moved, repository), /Unexpected download URL/);
});
