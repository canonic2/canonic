import test from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { docsHref, docsNavigation, isExternalLink, rewriteDocLink } from './docs.js';

const source = fileURLToPath(new URL('../../../workbench/docs/README.md', import.meta.url));
const shieldSource = fileURLToPath(new URL('../../../shield/docs/README.md', import.meta.url));

test('Markdown links point to published guides and preserve fragments and deployment base', () => {
  assert.equal(rewriteDocLink('configuration.md#scenarios', source), '/workbench/docs/configuration/#scenarios');
  assert.equal(docsHref('index', '/canonic'), '/canonic/workbench/docs/');
});

test('repository source links go to GitHub while external and fragment links stay intact', () => {
  assert.equal(rewriteDocLink('../README.md', source), 'https://github.com/canonic2/canonic/blob/main/packages/workbench/README.md');
  assert.equal(rewriteDocLink('../src/main.rs', shieldSource), 'https://github.com/canonic2/canonic/blob/main/packages/shield/src/main.rs');
  for (const url of ['#scenarios', 'https://example.com/docs.md', 'mailto:acme@example.com', '/absolute/path']) {
    assert.equal(rewriteDocLink(url, source), url);
  }
});

test('Shield guides and cross-product links use their own routes and deployment base', () => {
  assert.equal(rewriteDocLink('getting-started.md#install', shieldSource, '/canonic/'), '/canonic/shield/docs/getting-started/#install');
  assert.equal(rewriteDocLink('README.md', shieldSource), '/shield/docs/');
  assert.equal(rewriteDocLink('../../workbench/docs/configuration.md#scenarios', shieldSource), '/workbench/docs/configuration/#scenarios');
  assert.equal(docsHref('nested/index', '/', 'shield'), '/shield/docs/nested/');
  const outside = fileURLToPath(new URL('../../README.md', import.meta.url));
  assert.equal(rewriteDocLink('getting-started.md', outside), 'getting-started.md');
});

test('published Markdown links stay in the current locale across guides and products', () => {
  const options = { relativeGuides: true };
  const origin = 'https://example.com/canonic/ja/shield/docs/';
  const within = rewriteDocLink('getting-started.md#install', shieldSource, '/canonic/', options);
  const cross = rewriteDocLink('../../workbench/docs/configuration.md#scenarios', shieldSource, '/canonic/', options);
  assert.equal(new URL(within, origin).pathname, '/canonic/ja/shield/docs/getting-started/');
  assert.equal(new URL(cross, origin).href, 'https://example.com/canonic/ja/workbench/docs/configuration/#scenarios');
  assert.equal(rewriteDocLink('README.md', shieldSource, '/canonic/', options), './');
});

test('navigation accepts a product-specific order and includes unlisted guides', () => {
  const entries = [{ id: 'index', body: '# Shield' }, { id: 'extra', body: '# Extra' }, { id: 'install', body: '# Install' }];
  const groups = docsNavigation(entries, [{ title: 'Start', ids: ['install', 'index', 'absent'] }]);
  assert.deepEqual(groups.map(group => group.entries.map(entry => entry.id)), [['install', 'index'], ['extra']]);
});

test('only links to another origin count as external', () => {
  const site = 'https://example.com';
  assert.equal(isExternalLink('https://github.com/canonic2/canonic', site), true);
  assert.equal(isExternalLink('http://example.org/', site), true);
  for (const url of ['https://example.com/workbench/docs/', '/workbench/docs/', '#scenarios', 'mailto:acme@example.com', undefined]) {
    assert.equal(isExternalLink(url, site), false);
  }
});

test('new Markdown guides are included in navigation without frontmatter', () => {
  const entries = [
    { id: 'canvas', body: '# Using the canvas' },
    { id: 'index', body: '# Workbench documentation' },
    { id: 'new-guide', body: '# New guide' },
  ];
  const navigation = docsNavigation(entries);
  assert.equal(navigation.at(-1).title, 'More guides');
  assert.deepEqual(navigation[0].entries.map(entry => entry.id), ['index', 'canvas']);
  assert.equal(navigation.at(-1).entries[0].id, 'new-guide');
});
