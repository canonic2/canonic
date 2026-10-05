import { test } from 'node:test';
import assert from 'node:assert/strict';
import { exportSource } from './export-source.ts';

const source = [
  "import { Card } from './card.tsx';",
  '',
  '/** The surface alone. */',
  'export const basic = () => <Card>This is a card</Card>;',
  '',
  'export const withNestedContent = () => (',
  '  <Card>',
  "    <h4>The card's title</h4>",
  '',
  '    <p>More complex children.</p>',
  '  </Card>',
  ');',
  '',
  '// Belongs to the next export.',
  'export function WithCustomStyle() {',
  "  const style = { borderColor: '#0b72ff' };",
  '  return <Card style={style}>Custom</Card>;',
  '}',
  '',
  'function large() {',
  '  return <Card>Large</Card>;',
  '}',
  '',
  'export { large as sizeLarge };',
  "export { small } from './sizes.tsx';",
].join('\n');

test('a single-line export comes with the comment above it', () => {
  assert.equal(exportSource(source, 'basic'), '/** The surface alone. */\nexport const basic = () => <Card>This is a card</Card>;');
});

test('a multi-line export ends at its closing line, through blank lines and apostrophes in JSX', () => {
  assert.equal(
    exportSource(source, 'withNestedContent'),
    ['export const withNestedContent = () => (', '  <Card>', "    <h4>The card's title</h4>", '', '    <p>More complex children.</p>', '  </Card>', ');'].join('\n'),
  );
});

test('a function export keeps its own leading comment, not the previous statement', () => {
  assert.equal(
    exportSource(source, 'WithCustomStyle'),
    ['// Belongs to the next export.', 'export function WithCustomStyle() {', "  const style = { borderColor: '#0b72ff' };", '  return <Card style={style}>Custom</Card>;', '}'].join('\n'),
  );
});

test('an export list points at the local declaration; a re-export shows its line', () => {
  assert.equal(exportSource(source, 'sizeLarge'), ['function large() {', '  return <Card>Large</Card>;', '}'].join('\n'));
  assert.equal(exportSource(source, 'small'), "export { small } from './sizes.tsx';");
  assert.equal(exportSource(source, 'missing'), null);
});
