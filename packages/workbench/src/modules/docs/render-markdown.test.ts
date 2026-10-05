import { test } from 'node:test';
import assert from 'node:assert/strict';
import { renderDocs, resolveAgainst, type PanelView } from './render-markdown.ts';
import type { ExamplePlacement } from './docs-markdown.ts';

const source = [
  '---',
  'title: ignored',
  '---',
  '# Card `div`',
  '',
  'A plain surface. See [Button](../button/button.md#sizes), [the guide](guide.html), and [home](https://example.com).',
  '',
  '![Anatomy](images/anatomy.png "Parts")',
  '',
  '## Basic',
  '',
  '```example basic',
  'caption: children',
  '```',
  '',
  '## Basic',
  '',
  '```example nested',
  '```',
  '',
  '```example basic',
  '```',
  '',
  '| Prop | Type |',
  '| --- | --- |',
  '| `children` | `ReactNode` |',
  '',
  '```ts',
  'const card: string = "<Card>";',
  '```',
].join('\n');

function render(panel: (placement: ExamplePlacement) => PanelView) {
  return renderDocs({ source, file: 'docs/card/card.md', docsPages: new Set(['docs/button/button.md']), panel });
}

test('example blocks become panels in the state the lens gives them, followed by their caption', () => {
  const seen: string[] = [];
  const rendered = render(placement => {
    seen.push(placement.id + (placement.duplicate ? ' (again)' : ''));
    if (placement.duplicate) return { status: 'invalid', note: 'basic is placed more than once.' };
    return placement.id === 'basic' ? { status: 'ready' } : { status: 'missing', note: 'Not available in Dark' };
  });
  assert.deepEqual(seen, ['basic', 'nested', 'basic (again)']);
  const html = rendered.html;
  assert.match(html, /<figure class="wb-docs-example" id="example-basic" data-wb-example="basic" data-status="ready"><div class="wb-docs-example-stage" data-wb-example-stage><\/div>/);
  assert.match(html, /aria-label="Show code"[\s\S]*aria-label="Copy code"[\s\S]*<\/figure><p class="wb-docs-caption">children<\/p>/);
  assert.match(html, /data-wb-example="nested" data-status="missing"><p class="wb-docs-example-note">Not available in Dark<\/p><\/figure>/);
  assert.match(html, /data-status="invalid"><p class="wb-docs-example-note">basic is placed more than once.<\/p>/);
  assert.doesNotMatch(html, /title: ignored|caption: children/);
});

test('headings get unique anchors and the first one titles the page', () => {
  const rendered = render(() => ({ status: 'ready' }));
  assert.match(rendered.html, /<h1 id="card-div">Card <code>div<\/code><\/h1>/);
  assert.match(rendered.html, /<h2 id="basic">Basic<\/h2>[\s\S]*<h2 id="basic-1">Basic<\/h2>/);
  assert.equal(rendered.title, 'Card div');
});

test('relative links and images resolve against the Markdown file; links to docs pages are marked', () => {
  const html = render(() => ({ status: 'ready' })).html;
  assert.match(html, /<a href="\/docs\/button\/button.md#sizes" data-wb-docs-page="docs\/button\/button.md">Button<\/a>/);
  assert.match(html, /<a href="\/docs\/card\/guide.html">the guide<\/a>/);
  assert.match(html, /<a href="https:\/\/example.com">home<\/a>/);
  assert.match(html, /<img src="\/docs\/card\/images\/anatomy.png" alt="Anatomy" title="Parts">/);
});

test('tables render and fenced code is highlighted and escaped', () => {
  const html = render(() => ({ status: 'ready' })).html;
  assert.match(html, /<table>[\s\S]*<code>children<\/code>[\s\S]*<\/table>/);
  assert.match(html, /<pre class="wb-docs-code"><code class="language-ts"><span class="hljs-keyword">const<\/span>/);
  assert.match(html, /&#34;&lt;Card&gt;&#34;|&quot;&lt;Card&gt;&quot;/);
});

test('relative addresses never climb above the project root', () => {
  assert.equal(resolveAgainst('docs/card.md', '../../outside.png'), '../../outside.png');
  assert.equal(resolveAgainst('docs/card.md', './a b.png?x=1'), '/docs/a%20b.png?x=1');
  assert.equal(resolveAgainst('docs/card.md', '#props'), '#props');
  assert.equal(resolveAgainst('card.md', 'mailto:hi@example.com'), 'mailto:hi@example.com');
});
