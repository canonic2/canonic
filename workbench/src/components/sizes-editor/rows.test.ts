import test from 'node:test';
import assert from 'node:assert/strict';
import { draftOf, rowsProblem, rowValue } from './rows.ts';
import { defaultSizes } from '../../sizes/browser/size.ts';
import type { ResolvedSize } from '../../sizes/browser/size.ts';

const [fit, laptop] = defaultSizes() as [ResolvedSize, ResolvedSize];
const sidebar: ResolvedSize = { key: 'sidebar', label: 'Sidebar', icon: 'panel-left', button: true, kind: 'fixed', width: 340, height: 'fill' };
const tablet: ResolvedSize = { key: 'tablet', label: 'Tablet', icon: 'frame', button: false, kind: 'fixed', width: 1024, height: 1366 };

test('a row left as it was writes back what workbench.yaml would hold', () => {
  assert.deepEqual(rowValue(draftOf(fit)), { key: 'fit', value: true });
  assert.deepEqual(rowValue(draftOf(laptop)), { key: 'laptop', value: true });
  assert.deepEqual(rowValue(draftOf(sidebar)), { key: 'sidebar', value: { width: 340, height: 'fill', icon: 'panel-left', button: true } });
  assert.deepEqual(rowValue(draftOf(tablet)), { key: 'tablet', value: { width: 1024, height: 1366 } });
});

test('an edited row writes only the fields that differ from the default or the key', () => {
  assert.deepEqual(rowValue({ ...draftOf(laptop), name: 'MacBook', width: '1440', button: false }),
    { key: 'laptop', value: { label: 'MacBook', width: 1440, button: false } });
  assert.deepEqual(rowValue({ ...draftOf(fit), icon: 'maximize' }), { key: 'fit', value: { icon: 'maximize' } });
  assert.deepEqual(rowValue({ ...draftOf(tablet), name: 'iPad', icon: '' }), { key: 'tablet', value: { label: 'iPad', width: 1024, height: 1366 } });
});

test('rows that can’t be written are named, local rows aren’t checked, and a space keeps a size', () => {
  assert.equal(rowsProblem([draftOf(fit), draftOf(sidebar)]), null);
  assert.equal(rowsProblem([]), 'A space keeps at least one size.');
  assert.equal(rowsProblem([{ ...draftOf(tablet), name: '' }]), 'tablet needs a name.');
  assert.match(rowsProblem([{ ...draftOf(tablet), widthFill: true, heightFill: true }])!, /^Tablet: width and height can’t both fill/);
  assert.match(rowsProblem([{ ...draftOf(tablet), height: '9000' }])!, /^Tablet: height must be/);
  assert.match(rowsProblem([{ ...draftOf(tablet), icon: 'Bad' }])!, /kebab-case/);
  assert.equal(rowsProblem([{ ...draftOf({ ...tablet, local: true }), name: '' }]), null);
});
