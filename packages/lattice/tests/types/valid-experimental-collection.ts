import { createCollection } from '@canonic2/lattice/experimental/collection';
import { createCollection as shipping } from '@canonic2/lattice/collection';

const tasks = createCollection([['a', { label: 'A', done: false }]]);
tasks.updateMembership({ remove: ['a'], insert: [['b', { label: 'B', done: true }]] });
tasks.updateMembership({ insert: [['c', { label: 'C', done: false }, 'b']] });
tasks.watchItem('b', { fields: ['done'], read: (item) => item.done }, (value) => {
  const checked: boolean = value;
  void checked;
});
const text: string | undefined = tasks.get('b')?.label;
void text;

// @ts-expect-error Inserted records retain the collection's inferred item shape.
tasks.updateMembership({ insert: [['x', { label: 'Incomplete' }]] });
// @ts-expect-error Removal keys are strings.
tasks.updateMembership({ remove: [1] });
// @ts-expect-error Anchors are strings.
tasks.updateMembership({ insert: [['x', { label: 'X', done: false }, 1]] });
// @ts-expect-error Unknown membership-plan options are not accepted.
tasks.updateMembership({ reorder: ['a'] });
// @ts-expect-error The regular collection has no experimental operation.
shipping([['a', { label: 'A', done: false }]]).updateMembership({});
