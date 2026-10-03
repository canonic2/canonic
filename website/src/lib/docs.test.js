import test from 'node:test';
import assert from 'node:assert/strict';
import { fileURLToPath } from 'node:url';
import { docsHref, docsNavigation, rewriteDocLink } from './docs.js';

const source = fileURLToPath(new URL('../../../workbench/docs/README.md', import.meta.url));
const spec = fileURLToPath(new URL('../../../workbench/docs/specs/core.md', import.meta.url));

test('Markdown links point to published guides and preserve fragments and deployment base', () => {
  assert.equal(rewriteDocLink('configuration.md#states', source), '/workbench/docs/configuration/#states');
  assert.equal(rewriteDocLink('specs/README.md', source, '/canonic/'), '/canonic/workbench/docs/specs/');
  assert.equal(rewriteDocLink('../README.md#start-here', spec), '/workbench/docs/#start-here');
  assert.equal(rewriteDocLink('capture.md', spec), '/workbench/docs/specs/capture/');
  assert.equal(docsHref('index', '/canonic'), '/canonic/workbench/docs/');
});

test('repository source links go to GitHub while external and fragment links stay intact', () => {
  assert.equal(rewriteDocLink('../workbench/README.md', source), 'https://github.com/canonic2/canonic/blob/main/workbench/workbench/README.md');
  assert.equal(rewriteDocLink('../../workbench/markup.js', spec), 'https://github.com/canonic2/canonic/blob/main/workbench/workbench/markup.js');
  for (const url of ['#states', 'https://example.com/docs.md', 'mailto:acme@example.com', '/absolute/path']) {
    assert.equal(rewriteDocLink(url, source), url);
  }
});

test('new Markdown guides are included in navigation without frontmatter', () => {
  const entries = [
    { id: 'canvas', body: '# Using the canvas' },
    { id: 'index', body: '# Workbench documentation' },
    { id: 'new-guide', body: '# New guide' },
  ];
  const navigation = docsNavigation(entries);
  assert.deepEqual(navigation[0].entries.map(entry => entry.id), ['index', 'canvas']);
  assert.equal(navigation.at(-1).entries[0].id, 'new-guide');
});
