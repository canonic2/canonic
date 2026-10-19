import test from 'node:test';
import assert from 'node:assert/strict';
import { createStoryCatalog } from './story-catalog.js';

void test('one catalog coalesces concurrent shells and reuses each title independently', async () => {
  const calls = [];
  const catalog = createStoryCatalog(async (implementation, title) => {
    calls.push([implementation, title]);
    return [title];
  });
  const first = catalog.stories('storybook', 'Components/Button');
  assert.equal(catalog.stories('storybook', 'Components/Button'), first);
  assert.deepEqual(await first, ['Components/Button']);
  await catalog.stories('storybook', 'Components/Button');
  await catalog.stories('storybook', 'Components/Inputs');
  await catalog.stories('other', 'Components/Button');
  assert.equal(calls.length, 3);
});

void test('failed lookups retry and refresh replaces the cache without old failures deleting new requests', async () => {
  let rejectOld,
    calls = 0;
  const catalog = createStoryCatalog(async () => {
    calls++;
    if (calls === 1)
      return new Promise((_resolve, reject) => {
        rejectOld = reject;
      });
    if (calls === 2) throw new Error('unavailable');
    return ['ready'];
  });
  const old = catalog.stories('storybook', 'Button');
  await Promise.resolve();
  catalog.clear();
  await assert.rejects(catalog.stories('storybook', 'Button'), /unavailable/);
  const next = catalog.stories('storybook', 'Button');
  rejectOld(new Error('old failure'));
  await assert.rejects(old, /old failure/);
  assert.deepEqual(await next, ['ready']);
  assert.equal(catalog.stories('storybook', 'Button'), next);
  assert.equal(calls, 3);
});
