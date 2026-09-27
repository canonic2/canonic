const assert = require('node:assert/strict');
const fs = require('node:fs');
const path = require('node:path');
const test = require('node:test');
const { renderDownloads, selectRelease } = require('./build.cjs');

const html = fs.readFileSync(path.join(__dirname, 'index.html'), 'utf8');
const repository = 'canonic2/canonic';
const targets = ['darwin-arm64', 'darwin-x64', 'win32-x64', 'win32-arm64', 'linux-x64', 'linux-arm64'];

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
  const output = renderDownloads(html, release('1.0.1'), repository);
  assert.match(output, /workbench v1\.0\.1 on GitHub/);
  for (const target of targets) {
    assert.ok(output.includes(`releases/download/workbench%2Fv1.0.1/canonic-workbench-${target}.vsix`));
  }
  assert.ok(!output.includes('/releases/latest/download/'));
});

test('a website tag can publish before the first workbench release', () => {
  const output = renderDownloads(html, null, repository);
  assert.match(output, /after the first successful workbench release/);
  assert.ok(!output.includes('/releases/latest/download/'));
});

test('the newest complete workbench release wins', () => {
  const newest = release('1.0.2');
  assert.equal(selectRelease([release('1.0.1'), release('1.0.3', 'linux-arm64'), newest]), newest);
  assert.throws(() => renderDownloads(html, release('1.0.3', 'linux-arm64'), repository), /missing/);
});
