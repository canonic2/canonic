import test from 'node:test';
import assert from 'node:assert/strict';
import { keyForName, readPageSizes, readSpaceSizes } from './schema.ts';
import { defaultSizes } from './browser/size.ts';

test('a space without sizes, or with none usable, gets the default sizes', () => {
  const problems: string[] = [];
  assert.deepEqual(readSpaceSizes(undefined, problems), defaultSizes());
  assert.deepEqual(problems, []);
  assert.deepEqual(readSpaceSizes({ wide: { width: 'fill', height: 'fill' } }, problems).map(size => size.key), ['fit', 'laptop', 'mobile', 'resizable']);
  assert.deepEqual(problems, [
    'Sizes › wide: width and height can’t both be fill; use fit.',
    'Sizes: no size can be used, so the default sizes are.',
  ]);
});

test('a space declares exactly its sizes, in order, with defaults by true and fields that override them', () => {
  const problems: string[] = [];
  const sizes = readSpaceSizes({
    fit: true,
    sidebar: { label: 'Sidebar', width: 340, height: 'fill', icon: 'panel-left', button: true },
    laptop: { label: 'MacBook', width: 1440 },
    tablet: { width: 1024, height: 1366 },
  }, problems, ['tablet']);
  assert.deepEqual(problems, []);
  assert.deepEqual(sizes.map(size => size.key), ['fit', 'sidebar', 'laptop', 'tablet']);
  assert.deepEqual(sizes[1], { key: 'sidebar', label: 'Sidebar', icon: 'panel-left', button: true, kind: 'fixed', width: 340, height: 'fill' });
  assert.deepEqual(sizes[2], { key: 'laptop', label: 'MacBook', icon: 'monitor', button: true, kind: 'fixed', width: 1440, height: 982 });
  assert.deepEqual(sizes[3], { key: 'tablet', label: 'Tablet', icon: 'frame', button: false, kind: 'fixed', width: 1024, height: 1366, local: true });
});

test('a space’s malformed sizes are reported and left out, the rest kept', () => {
  const problems: string[] = [];
  const sizes = readSpaceSizes({
    '1024': { width: 1024, height: 768 },
    'Big One': { width: 10, height: 10 },
    fit: { width: 300 },
    sidebar: true,
    tiny: { width: 0, height: 10 },
    huge: { width: 9000, height: 10 },
    odd: { width: 10.5, height: 10 },
    banner: { width: 'fill', height: 90, depth: 2, icon: 'Bad Icon', button: 'yes' },
    popover: 'small',
  }, problems);
  assert.deepEqual(sizes.map(size => size.key), ['banner']);
  assert.deepEqual(sizes[0], { key: 'banner', label: 'Banner', icon: 'frame', button: false, kind: 'fixed', width: 'fill', height: 90 });
  assert.deepEqual(problems, [
    'Sizes: “1024” must be a kebab-case key that isn’t all digits.',
    'Sizes: “Big One” must be a kebab-case key that isn’t all digits.',
    'Sizes › fit: fit fills the canvas, so it takes no width or height.',
    'Sizes › sidebar: true only includes a default size (fit, laptop, mobile, or resizable); give this size a width and height.',
    'Sizes › tiny: width must be a whole number from 1 to 8192, or fill.',
    'Sizes › huge: width must be a whole number from 1 to 8192, or fill.',
    'Sizes › odd: width must be a whole number from 1 to 8192, or fill.',
    'Sizes › banner: “depth” isn’t a size field (width, height, label, icon, button).',
    'Sizes › banner: icon “Bad Icon” must be a kebab-case Lucide icon name.',
    'Sizes › banner: button must be true or false.',
    'Sizes › popover: must be true or a map of width, height, label, icon, and button.',
  ]);
});

test('a page limits the space’s sizes and adds its own, which can’t change the space’s or be buttons', () => {
  const space = readSpaceSizes({ fit: true, sidebar: { width: 340, height: 'fill' }, mobile: true }, []);
  const problems: string[] = [];
  const where = 'Components › Account menu';
  assert.equal(readPageSizes(undefined, space, where, problems), null);
  assert.deepEqual(readPageSizes('mobile', space, where, problems), { sizes: ['mobile'] });
  const page = readPageSizes([
    { popover: { width: 280, height: 360, icon: 'panel-top', button: true } },
    'sidebar',
    'sidebar',
    'tablet',
    { sidebar: { width: 300 } },
    { a: { width: 1, height: 1 }, b: { width: 1, height: 1 } },
  ], space, where, problems);
  assert.deepEqual(page, {
    sizes: ['popover', 'sidebar'],
    ownSizes: [{ key: 'popover', label: 'Popover', icon: 'panel-top', button: false, kind: 'fixed', width: 280, height: 360 }],
  });
  assert.deepEqual(problems, [
    'Components › Account menu › popover: button doesn’t apply to a page’s own size; the size switcher’s buttons belong to the space.',
    'Components › Account menu: size “tablet” isn’t one of the space’s sizes.',
    'Components › Account menu: size “sidebar” is one of the space’s sizes; a page can list it but not change it.',
    'Components › Account menu: each size is a key of the space’s sizes, or one key with the fields of a size of the page’s own.',
  ]);
});

test('a page with no usable size supports every size of the space', () => {
  const problems: string[] = [];
  assert.equal(readPageSizes(['tablet'], defaultSizes(), 'Pages › Sign in', problems), null);
  assert.equal(problems.length, 1);
});

test('a new size’s key comes from its name and avoids the keys already taken', () => {
  assert.equal(keyForName('Small phone', []), 'small-phone');
  assert.equal(keyForName('Écran large!', []), 'ecran-large');
  assert.equal(keyForName('1024', []), 'size-1024');
  assert.equal(keyForName('  ', []), 'size');
  assert.equal(keyForName('Tablet', ['tablet', 'tablet-2']), 'tablet-3');
});
