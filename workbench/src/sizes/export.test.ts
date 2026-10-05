import test from 'node:test';
import assert from 'node:assert/strict';
import { exportSizes } from './export.ts';
import { defaultSizes } from './browser/size.ts';
import type { ResolvedSize } from './browser/size.ts';

const [fit, laptop, mobile, resizable] = defaultSizes() as [ResolvedSize, ResolvedSize, ResolvedSize, ResolvedSize];
const sidebar: ResolvedSize = { key: 'sidebar', label: 'Sidebar', icon: 'panel-left', button: true, kind: 'fixed', width: 340, height: 'fill' };
const wide: ResolvedSize = { key: 'wide', label: 'Wide', icon: 'frame', button: false, kind: 'fixed', width: 1440, height: 900 };

test('a page is captured at each size it supports but Resizable, in its order', () => {
  assert.deepEqual(exportSizes([sidebar, resizable, laptop]), [
    { key: 'sidebar', label: 'Sidebar', width: 340, height: 900 },
    { key: 'laptop', label: 'Laptop', width: 1512, height: 982 },
  ]);
});

test('sizes that come to the same dimensions are captured once, under the first', () => {
  assert.deepEqual(exportSizes([fit, wide, mobile]).map(size => size.key), ['fit', 'mobile']);
});

test('a page whose only size is Resizable is still captured once, at 1440 × 900', () => {
  assert.deepEqual(exportSizes([resizable]), [{ key: 'resizable', label: 'Resizable', width: 1440, height: 900 }]);
});
