import test from 'node:test';
import assert from 'node:assert/strict';
import { choose, describe, supported } from './choice.ts';
import { accepts, clamp, dimensions, exportDimensions, fills } from './geometry.ts';
import { defaultSizes, isSizeKey } from './size.ts';
import type { ResolvedSize } from './size.ts';

const sidebar: ResolvedSize = { key: 'sidebar', label: 'Sidebar', icon: 'panel-left', button: true, kind: 'fixed', width: 340, height: 'fill' };
const banner: ResolvedSize = { key: 'banner', label: 'Banner', icon: 'frame', button: false, kind: 'fixed', width: 'fill', height: 90 };
const popover: ResolvedSize = { key: 'popover', label: 'Popover', icon: 'frame', button: false, kind: 'fixed', width: 280, height: 360 };
const space = [...defaultSizes(), sidebar];
const [fit, laptop, mobile, resizable] = space as [ResolvedSize, ResolvedSize, ResolvedSize, ResolvedSize];
const room = { width: 1200, height: 812 };
const dragged = { width: 640, height: 480 };

test('a filled axis takes the canvas’s room, never below the floor; a fixed axis keeps its length', () => {
  assert.deepEqual(dimensions(sidebar, room, dragged), { width: 340, height: 812 });
  assert.deepEqual(dimensions(banner, room, dragged), { width: 1200, height: 90 });
  assert.deepEqual(dimensions(fit, room, dragged), room);
  assert.deepEqual(dimensions(fit, { width: 100, height: 20 }, dragged), { width: 320, height: 320 });
  assert.deepEqual(dimensions(laptop, room, dragged), { width: 1512, height: 982 });
  assert.deepEqual(dimensions(resizable, room, dragged), dragged);
  assert.deepEqual(dimensions(resizable, room, { width: 0, height: 9000 }), { width: 1, height: 8192 });
  assert.deepEqual(fills(sidebar), { width: false, height: true });
  assert.deepEqual(fills(fit), { width: true, height: true });
});

test('an export captures a filled axis, and Fit, at 1440 × 900, and Resizable not at all', () => {
  assert.deepEqual(exportDimensions(sidebar), { width: 340, height: 900 });
  assert.deepEqual(exportDimensions(fit), { width: 1440, height: 900 });
  assert.equal(exportDimensions(resizable), null);
});

test('a size accepts an artboard whose fixed axes match exactly; Fit and Resizable accept any', () => {
  assert.ok(accepts(sidebar, { width: 340, height: 1 }));
  assert.ok(!accepts(sidebar, { width: 341, height: 812 }));
  assert.ok(accepts(mobile, { width: 393, height: 852 }));
  assert.ok(!accepts(mobile, { width: 393, height: 851 }));
  assert.ok(accepts(fit, { width: 3, height: 4 }) && accepts(resizable, { width: 3, height: 4 }));
  assert.deepEqual(clamp({ width: 0.4, height: Number.NaN }), { width: 1, height: 1 });
});

test('a page supports its listed sizes in its order, its own included, or every size of the space', () => {
  assert.deepEqual(supported(space, null), space);
  assert.deepEqual(supported(space, { sizes: ['sidebar', 'fit'] }), [sidebar, fit]);
  assert.deepEqual(supported(space, { sizes: ['popover', 'mobile'], ownSizes: [popover] }), [popover, mobile]);
  assert.deepEqual(supported(space, { sizes: ['gone'] }), space, 'nothing left means every size');
});

test('the shown size is the first wanted one the page supports, else its first', () => {
  const page = supported(space, { sizes: ['sidebar', 'resizable'] });
  assert.equal(choose(page, 'resizable', 'fit', 'mobile'), 'resizable');
  assert.equal(choose(page, null, 'fit', 'resizable'), 'resizable');
  assert.equal(choose(page, 'laptop', undefined, null), 'sidebar');
  assert.equal(choose([], 'fit'), null);
});

test('a size is described by its label and declared dimensions', () => {
  assert.equal(describe(sidebar), 'Sidebar, 340 × fill');
  assert.equal(describe(mobile), 'Mobile, 393 × 852');
  assert.equal(describe(fit), 'Fit');
  assert.equal(describe(resizable), 'Resizable');
});

test('size keys are kebab-case and never all digits, so they can’t be read as widths', () => {
  assert.ok(isSizeKey('small-phone') && isSizeKey('ipad-11'));
  assert.ok(!isSizeKey('1512') && !isSizeKey('Small') && !isSizeKey('a--b') && !isSizeKey('a/b') && !isSizeKey(''));
});
