import { test } from 'node:test';
import assert from 'node:assert/strict';
import { readRequest, selectView } from './request.ts';
import { selectCaptures } from './selection.ts';
import { pdfOutput } from './output.ts';
import { PDFDocument } from 'pdf-lib';
test('selected pages keep their collection hierarchy and reject foreign pages', () => {
  const view = { pages: { a: { label: 'A' }, b: { label: 'B' } }, collections: [{ name: 'Pages', items: [{ label: 'Group', items: [{ src: 'a' }, { src: 'b' }] }] }] };
  const selected = selectView(view, readRequest({ scope: 'pages', pages: ['b'] }));
  assert.deepEqual(Object.keys(selected.pages), ['b']);
  assert.deepEqual(selected.collections[0].items[0].items, [{ src: 'b' }]);
  assert.throws(() => selectView(view, readRequest({ scope: 'page', pages: ['foreign'] })), /does not belong/);
});
test('visual page defaults use the current state and actual dimensions exactly once', () => {
  const request = readRequest({ format: 'pdf', scope: 'page', current: { page: 'a', state: null, width: 777, height: 600 } });
  assert.equal(request.variants, 'current');
  const captures = ['wide', 'mobile'].flatMap(size => ['first', 'error'].map(state => ({ page: 'a', state, size, width: 100, height: 100, variant: state + size, url: 'http://localhost/a' })));
  const selected = selectCaptures(captures, request);
  assert.equal(selected.length, 1); assert.equal(selected[0].state, 'first'); assert.equal(selected[0].width, 777);
  assert.equal(readRequest({ scope: 'page', pages: ['a'] }).variants, 'all');
});
test('PDF merging preserves custom visual and standard documentation page sizes', async () => {
  const references = [];
  for (const size of [[600, 4000], [595, 842], [612, 792]]) {
    const doc = await PDFDocument.create(); doc.addPage(size as [number, number]);
    references.push({ page: 'a', label: 'Page', body: Buffer.from(await doc.save()) });
  }
  const result = await pdfOutput('Designs', readRequest({ format: 'pdf' }), references, ['One unavailable page']);
  const merged = await PDFDocument.load(result.download.body);
  assert.deepEqual(merged.getPages().map(p => [p.getWidth(), p.getHeight()]), [[600, 4000], [595, 842], [612, 792]]);
  assert.equal(result.download.contentType, 'application/pdf'); assert.equal(result.report.warnings.length, 1);
});
