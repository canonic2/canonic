/** @file Browser acceptance cases for the regular package's final membership candidate. */
import { expect, test, vi } from 'vitest';
import { createCollection } from '@canonic2/lattice/collection';

test('bulk replacement owns native signal cleanup and nested rollback in Chromium', () => {
  const c = createCollection([
    ['a', { count: 1 }],
    ['b', { count: 2 }],
  ]);
  const owner = new AbortController(),
    run = vi.fn();
  const detach = vi.spyOn(owner.signal, 'removeEventListener');
  const before = c.keys(),
    original = c.get('a');
  c.watchItem('a', { fields: ['count'], read: (item) => item.count }, run, {
    signal: owner.signal,
  });
  expect(() =>
    c.batch(() => {
      c.updateMembership({ remove: ['a'], insert: [['a', { count: 3 }, 'b']] });
      expect(c.get('a')).not.toBe(original);
      expect(detach).not.toHaveBeenCalled();
      throw new Error('rollback');
    }),
  ).toThrow('rollback');
  expect(c.keys()).toBe(before);
  expect(c.get('a')).toBe(original);
  const apply = c.updateMembership;
  apply({ remove: ['a'], insert: [['a', { count: 4 }, 'b']] });
  expect(detach).toHaveBeenCalledTimes(1);
  expect(run).toHaveBeenCalledTimes(1);
  expect(owner.signal.aborted).toBe(false);
  owner.abort();
  c.update('a', { count: 5 });
  expect(run).toHaveBeenCalledTimes(1);
  detach.mockRestore();
});

test('bulk delivery stays synchronous, committed and selective in Chromium', () => {
  const c = createCollection([
    ['a', { count: 1 }],
    ['b', { count: 2 }],
  ]);
  const owner = new AbortController(),
    read = vi.fn((/** @type {{ count: number }} */ item) => item.count),
    run = vi.fn();
  c.watchItem('a', { fields: ['count'], read }, run, { signal: owner.signal });
  const keys = vi.fn();
  c.watchKeys(keys, { signal: owner.signal });
  read.mockClear();
  run.mockClear();
  keys.mockClear();
  c.batch(() => {
    c.updateMembership({
      remove: ['b'],
      insert: [
        ['x', { count: 3 }, 'a'],
        ['y', { count: 4 }, 'a'],
      ],
    });
    expect(c.keys()).toEqual(['x', 'y', 'a']);
    expect(keys).not.toHaveBeenCalled();
    c.update('a', { count: 5 });
  });
  expect(keys).toHaveBeenCalledTimes(1);
  expect(read).toHaveBeenCalledTimes(1);
  expect(run).toHaveBeenCalledWith(5, 1);
  owner.abort();
});
