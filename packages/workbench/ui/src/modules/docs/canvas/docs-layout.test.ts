import { test } from 'node:test';
import assert from 'node:assert/strict';
import { centeredX, docsLayout, scrollAfterZoom } from './docs-layout.ts';

const canvas = { width: 1200, height: 800 };

test('at 100% the page fills the canvas', () => {
  assert.deepEqual(docsLayout(canvas, 1, 0), { width: 1200, height: 800, x: 0, y: 0 });
});

test('zooming out keeps the layout width, centers the page, and shows more of it', () => {
  assert.deepEqual(docsLayout(canvas, 0.5, 0), { width: 1200, height: 1600, x: 300, y: 0 });
  assert.deepEqual(docsLayout(canvas, 0.5, -999), { width: 1200, height: 1600, x: 300, y: 0 }, 'a page that fits ignores the pan');
});

test('zoomed in past the canvas, the page pans horizontally within its edges', () => {
  assert.deepEqual(docsLayout(canvas, 2, -300), { width: 1200, height: 400, x: -300, y: 0 });
  assert.equal(docsLayout(canvas, 2, 50).x, 0, 'no white gap at the left edge');
  assert.equal(docsLayout(canvas, 2, -5000).x, -1200, 'no white gap at the right edge');
  assert.equal(centeredX(canvas, 2), -600);
});

test('zooming about a point keeps the page content under it', () => {
  // At 100%, a point 400px down shows page y = 1000 + 400 = 1400. At 50% the
  // same canvas point covers 800 page pixels, so the scroll becomes 600.
  assert.equal(scrollAfterZoom(1000, 400, 1, 0.5), 600);
  assert.equal(scrollAfterZoom(0, 400, 1, 0.5), 0, 'never above the top');
  assert.equal(scrollAfterZoom(600, 400, 0.5, 1), 1000);
});
