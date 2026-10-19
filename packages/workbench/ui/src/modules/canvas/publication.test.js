import test from 'node:test';
import assert from 'node:assert/strict';
import { createPublisher, spaceIds } from './publication.js';
import { createState } from './model.js';

void test('context reaches sibling servers sharing a root, but excludes other roots', () => {
  assert.deepEqual(
    [
      ...spaceIds({
        state: createState('canvas'),
        browsed: 'a',
        activity: 1,
        spaces: [
          { id: 'a', name: 'A', root: '/acme', url: '' },
          { id: 'b', name: 'B', root: '/acme', url: '' },
          { id: 'c', name: 'C', root: '/other', url: '' },
        ],
      }),
    ],
    ['a', 'b'],
  );
});

void test('publication serializes updates, preserves activity, withdraws old spaces and stops on disposal', async () => {
  let browsed = 'a',
    activity = 10;
  const sent = [];
  const withdrawn = [];
  const publisher = createPublisher({
    read: () => ({ state: createState('canvas'), browsed, spaces: [], activity }),
    async send(request) {
      sent.push(request);
    },
    withdraw(request) {
      withdrawn.push(request.id);
    },
    error(error) {
      throw error;
    },
  });
  try {
    await publisher.publish();
    browsed = 'b';
    activity = 11;
    await publisher.publish();
    assert.deepEqual(
      sent.map((r) => [r.id, !!r.body.closed, r.body.activity]),
      [
        ['a', false, 10],
        ['a', true, undefined],
        ['b', false, 11],
      ],
    );
  } finally {
    publisher.dispose();
  }
  await publisher.publish();
  assert.equal(sent.length, 3);
  assert.deepEqual(withdrawn, ['b']);
});
