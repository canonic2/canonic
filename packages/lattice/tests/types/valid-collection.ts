/** @file Consumer type fixture: uses of the collection entry point that must type-check. */
import { createCollection } from '@canonic2/lattice/collection';
import type {
  Collection,
  CollectionReader,
  CollectionSelection,
  CollectionView,
  ItemSelection,
  KeysChange,
  MembershipChanges,
  AggregateSelection,
} from '@canonic2/lattice/collection';
import { checkCollectionSelection, checkSelection } from '@canonic2/lattice/testing';

interface Task {
  label: string;
  done: boolean;
}

type IsAny<T> = 0 extends 1 & T ? true : false;
type Equal<A, B> =
  (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2 ? true : false;
type Expect<T extends true> = T;

const tasks = createCollection([['t1', { label: 'Plan', done: false }]]);

export type Imports = [
  Expect<Equal<IsAny<typeof createCollection>, false>>,
  Expect<Equal<IsAny<typeof tasks>, false>>,
  Expect<Equal<IsAny<ItemSelection<Task, string>>, false>>,
  Expect<Equal<IsAny<CollectionSelection<Task, number>>, false>>,
  Expect<Equal<IsAny<CollectionReader<Task>>, false>>,
  Expect<Equal<IsAny<typeof checkCollectionSelection>, false>>,
  Expect<Equal<IsAny<MembershipChanges<Task>>, false>>,
  Expect<Equal<IsAny<AggregateSelection<Task, number, number>>, false>>,
  Expect<Equal<Parameters<typeof tasks.updateMembership>[0], MembershipChanges<Task>>>,
];

// Generic inference: the collection is typed by its items.
const typed: Collection<Task> = tasks;
const reader: CollectionReader<Task> = tasks;
const detachedGet = reader.get;
detachedGet('t1');
// @ts-expect-error Readers expose no mutation methods.
reader.update('t1', { done: true });
// @ts-expect-error Readers expose no membership mutations.
reader.updateMembership({ remove: ['t1'] });
// @ts-expect-error Membership count is read-only.
reader.size = 10;
const record = typed.get('t1');
export type ItemRecord = Expect<
  Equal<typeof record, Readonly<{ label: string; done: boolean }> | undefined>
>;

tasks.update('t1', { label: 'Write' });
tasks.update('t1', (task) => ({ done: !task.done }));
tasks.insert('t2', { label: 'Review', done: false }, 't1');
tasks.batch(() => {
  tasks.remove('t2');
});
const membership = tasks.updateMembership;
const changes = {
  remove: ['t1'],
  insert: [['t1', { label: 'Replacement', done: false }, undefined]],
} as const satisfies MembershipChanges<Task>;
membership(changes);
// @ts-expect-error Removal keys must be strings.
membership({ remove: [42] });
// @ts-expect-error Insertions preserve the inferred record shape.
membership({ insert: [['wrong', { label: 42, done: false }]] });
// @ts-expect-error Anchors must be strings or undefined.
membership({ insert: [['wrong', { label: 'Wrong', done: false }, 42]] });
// @ts-expect-error Insertion entries need a record as well as a key.
membership({ insert: [['wrong']] });

// An item selection checked by `satisfies`: read receives the item type.
const label = { fields: ['label'], read: (task) => task.label } satisfies ItemSelection<
  Task,
  string
>;
export type ReadItem = Expect<Equal<Parameters<typeof label.read>[0], Readonly<Task>>>;

tasks.watchItem('t1', label, (value, previous) => {
  const current: Expect<Equal<typeof value, string>> = true;
  const before: Expect<Equal<typeof previous, string | undefined>> = true;
  void current;
  void before;
});

const open = {
  fields: ['done'],
  read: (view) => {
    const keys = view.keys();
    let count = 0;
    for (let index = 0; index < keys.length; index++)
      if (!view.get(keys[index] ?? '')?.done) count++;
    return count;
  },
} satisfies CollectionSelection<Task, number>;
export type ReadView = Expect<Equal<Parameters<typeof open.read>[0], CollectionView<Task>>>;
const checked = checkCollectionSelection('open', open, { current: reader });
export type CheckedFields = Expect<Equal<typeof checked.accessed, readonly string[]>>;
checkSelection('label', label, { one: { label: 'Acme', done: false } });
const traversal: CollectionView<Task> = reader;
// @ts-expect-error Derivations cannot change their traversal view.
traversal.size = 10;

tasks.watch(open, (count) => {
  const value: Expect<Equal<typeof count, number>> = true;
  void value;
});

const incrementalOpen = {
  fields: ['done'],
  read: (task) => (task.done ? 0 : 1),
  initial: () => 0,
  add: (total, contribution) => total + contribution,
  remove: (total, contribution) => total - contribution,
} satisfies AggregateSelection<Task, number, number>;
const detachedAggregate = reader.watchAggregate;
detachedAggregate(incrementalOpen, (value, previous) => {
  const current: Expect<Equal<typeof value, number>> = true;
  const before: Expect<Equal<typeof previous, number | undefined>> = true;
  void current;
  void before;
});
checkSelection('open contribution', incrementalOpen, { one: { label: 'Acme', done: false } });
const invalidAggregate = {
  ...incrementalOpen,
  // @ts-expect-error Aggregate declarations reject unknown item fields.
  fields: ['missing'],
} satisfies AggregateSelection<Task, number, number>;
void invalidAggregate;
tasks.watchAggregate(
  {
    fields: ['label'],
    read: (task) => task.label.length,
    initial: () => ({ total: 0, count: 0 }),
    add: (value, contribution) => ({ total: value.total + contribution, count: value.count + 1 }),
    remove: (value, contribution) => ({
      total: value.total - contribution,
      count: value.count - 1,
    }),
  },
  (value) => {
    const structured: Expect<Equal<typeof value, { total: number; count: number }>> = true;
    void structured;
  },
);

tasks.watchKeys(
  (keys, previous, change) => {
    const list: Expect<Equal<typeof keys, readonly string[]>> = true;
    const before: Expect<Equal<typeof previous, readonly string[] | undefined>> = true;
    const changed: Expect<Equal<typeof change, KeysChange | undefined>> = true;
    void list;
    void before;
    void changed;
  },
  { signal: new AbortController().signal },
);
