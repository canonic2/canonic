import test from 'node:test';
import assert from 'node:assert/strict';
import { addSize, updateSizes } from './edit.ts';

const collections = () => [{
  name: 'Interface',
  items: [
    { label: 'Sidebar', src: 'design/sidebar.html', sizes: ['sidebar', 'resizable'] },
    { group: 'Menus', items: [
      { label: 'Account menu', src: 'design/account-menu.html', sizes: ['sidebar', { popover: { width: 280, height: 360 } }] },
    ] },
    { label: 'Dashboard', src: 'pages/dashboard.html' },
  ],
}];

test('a size added to a space without sizes keeps the default sizes ahead of it', () => {
  const edit = addSize(undefined, collections(), { name: 'Tablet', width: 1024, height: 1366 });
  assert.equal(edit.key, 'tablet');
  assert.deepEqual(edit.sizes, { fit: true, laptop: true, mobile: true, resizable: true, tablet: { width: 1024, height: 1366 } });
  assert.equal(edit.collections, undefined, 'no page listed sizes, so none changes');
});

test('a size added from a page that lists sizes joins that list; its name becomes a label when it isn’t the key’s', () => {
  const edit = addSize({ fit: true, sidebar: { width: 340, height: 'fill' } }, collections(),
    { name: 'Wide panel!', width: 'fill', height: 200, icon: 'panel-top', button: true, page: 'design/sidebar.html' });
  assert.equal(edit.key, 'wide-panel');
  assert.deepEqual(edit.sizes!['wide-panel'], { label: 'Wide panel!', width: 'fill', height: 200, icon: 'panel-top', button: true });
  assert.deepEqual((edit.collections as any)[0].items[0].sizes, ['sidebar', 'resizable', 'wide-panel']);
});

test('a size only for one page is written to that page, with a key no page or space uses', () => {
  const edit = addSize({ fit: true, popover: { width: 1, height: 1 } }, collections(),
    { name: 'Popover', width: 300, height: 200, button: true, page: 'design/account-menu.html', pageOnly: true });
  assert.equal(edit.key, 'popover-2');
  assert.equal(edit.sizes, undefined, 'the space’s sizes are not written');
  assert.deepEqual((edit.collections as any)[0].items[1].items[0].sizes,
    ['sidebar', { popover: { width: 280, height: 360 } }, { 'popover-2': { label: 'Popover', width: 300, height: 200 } }]);
});

test('adding refuses what can’t be written', () => {
  assert.throws(() => addSize(undefined, [], { name: ' ', width: 10, height: 10 }), /name/);
  assert.throws(() => addSize(undefined, [], { name: 'Wide', width: 'fill', height: 'fill' }), /both be fill/);
  assert.throws(() => addSize(undefined, [], { name: 'Big', width: 9000, height: 10 }), /1 to 8192/);
  assert.throws(() => addSize(undefined, collections(), { name: 'Mine', width: 10, height: 10, page: 'pages/missing.html', pageOnly: true }), /listed in workbench.yaml/);
});

test('updating writes the sizes in the dialog’s order and removes a dropped size from every page', () => {
  const edit = updateSizes({ fit: true, sidebar: { width: 340, height: 'fill' }, resizable: true }, collections(), [
    { key: 'resizable', value: { label: 'Free' } },
    { key: 'fit', value: true },
  ]);
  assert.deepEqual(Object.keys(edit.sizes!), ['resizable', 'fit']);
  const items = (edit.collections as any)[0].items;
  assert.deepEqual(items[0].sizes, ['resizable']);
  assert.deepEqual(items[1].items[0].sizes, [{ popover: { width: 280, height: 360 } }]);
  assert.equal('sizes' in items[2], false);
});

test('a page left with no sizes loses its list, and so supports every size', () => {
  const edit = updateSizes({ fit: true, sidebar: { width: 340, height: 'fill' } }, [{ name: 'A', items: [{ label: 'S', src: 's.html', sizes: ['sidebar'] }] }],
    [{ key: 'fit', value: true }]);
  assert.equal('sizes' in (edit.collections as any)[0].items[0], false);
});

test('updating refuses the last size gone, unknown or repeated keys, invalid sizes, and changes to local sizes', () => {
  const sizes = { fit: true, sidebar: { width: 340, height: 'fill' } };
  assert.throws(() => updateSizes(sizes, [], []), /at least one size/);
  assert.throws(() => updateSizes(sizes, [], [{ key: 'tablet', value: { width: 1, height: 1 } }]), /no size “tablet”/);
  assert.throws(() => updateSizes(sizes, [], [{ key: 'fit', value: true }, { key: 'fit', value: true }]), /twice/);
  assert.throws(() => updateSizes(sizes, [], [{ key: 'sidebar', value: { width: 'fill', height: 'fill' } }]), /both be fill/);
  assert.throws(() => updateSizes(sizes, [], [{ key: 'fit', value: true }], ['sidebar']), /workbench.local.yaml; remove it there/);
});

test('a size set by the local file keeps its committed value and order, and one only the local file defines isn’t written', () => {
  const sizes = { fit: true, sidebar: { width: 340, height: 'fill' } };
  const edit = updateSizes(sizes, [], [
    { key: 'sidebar', value: { width: 360, height: 'fill', label: 'Local' } },
    { key: 'phone', value: { width: 1, height: 1 } },
    { key: 'fit', value: true },
  ], ['sidebar', 'phone']);
  assert.deepEqual(edit.sizes, { sidebar: { width: 340, height: 'fill' }, fit: true });
});
