import test from 'node:test';
import assert from 'node:assert/strict';
import { EMPTY_DRAFT, lengthOf, readDraft } from './form.ts';

const draft = (fields: Partial<typeof EMPTY_DRAFT>) => ({ ...EMPTY_DRAFT, ...fields });

test('a typed length is a whole number from 1 to 8192, or Fill', () => {
  assert.equal(lengthOf(' 340 ', false), 340);
  assert.equal(lengthOf('', true), 'fill');
  for (const bad of ['', '0', '8193', '12.5', '-3', '1e3', 'wide']) assert.equal(lengthOf(bad, false), null, bad);
});

test('the fields make the size the add route takes, or the first reason they can’t yet', () => {
  assert.deepEqual(readDraft(draft({ name: ' Sidebar ', width: '340', heightFill: true, icon: 'panel-left', button: true })),
    { size: { name: 'Sidebar', width: 340, height: 'fill', icon: 'panel-left', button: true, pageOnly: false } });
  assert.deepEqual(readDraft(draft({ name: 'Popover', width: '280', height: '360', button: true, pageOnly: true })),
    { size: { name: 'Popover', width: 280, height: 360, button: false, pageOnly: true } }, 'a page’s own size is never a button');
  assert.deepEqual(readDraft(draft({})), { problem: 'Give the size a name.' });
  assert.match((readDraft(draft({ name: 'A', width: '0', height: '1' })) as { problem: string }).problem, /^Width must be/);
  assert.match((readDraft(draft({ name: 'A', width: '1', height: '' })) as { problem: string }).problem, /^Height must be/);
  assert.match((readDraft(draft({ name: 'A', widthFill: true, heightFill: true })) as { problem: string }).problem, /use Fit/);
  assert.match((readDraft(draft({ name: 'A', width: '1', height: '1', icon: 'Panel Left' })) as { problem: string }).problem, /kebab-case/);
});
