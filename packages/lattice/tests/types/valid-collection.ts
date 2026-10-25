/** @file Consumer type fixture: uses of the collection entry point that must type-check. */
import { createCollection } from '@canonic2/lattice/collection';
import type {
  Collection,
  CollectionSelection,
  CollectionView,
  ItemSelection,
  KeysChange,
} from '@canonic2/lattice/collection';

interface Task {
  label: string;
  done: boolean;
}

type IsAny<T> = 0 extends 1 & T ? true : false;
type Equal<A, B> = (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2 ? true : false;
type Expect<T extends true> = T;

const tasks = createCollection([['t1', { label: 'Plan', done: false }]]);

export type Imports = [
  Expect<Equal<IsAny<typeof createCollection>, false>>,
  Expect<Equal<IsAny<typeof tasks>, false>>,
  Expect<Equal<IsAny<ItemSelection<Task, string>>, false>>,
  Expect<Equal<IsAny<CollectionSelection<Task, number>>, false>>,
];

// Generic inference: the collection is typed by its items.
const typed: Collection<Task> = tasks;
const record = typed.get('t1');
export type ItemRecord = Expect<Equal<typeof record, Readonly<{ label: string; done: boolean }> | undefined>>;

tasks.update('t1', { label: 'Write' });
tasks.update('t1', (task) => ({ done: !task.done }));
tasks.insert('t2', { label: 'Review', done: false }, 't1');
tasks.batch(() => {
  tasks.remove('t2');
});

// An item selection checked by `satisfies`: read receives the item type.
const label = { fields: ['label'], read: (task) => task.label } satisfies ItemSelection<Task, string>;
export type ReadItem = Expect<Equal<Parameters<typeof label.read>[0], Readonly<Task>>>;

tasks.watchItem('t1', label, (value, previous) => {
  const current: Expect<Equal<typeof value, string>> = true;
  const before: Expect<Equal<typeof previous, string | undefined>> = true;
  void current;
  void before;
});

const open = {
  items: ['done'],
  read: (view) => {
    const keys = view.keys();
    let count = 0;
    for (let index = 0; index < keys.length; index++) if (!view.get(keys[index] ?? '')?.done) count++;
    return count;
  },
} satisfies CollectionSelection<Task, number>;
export type ReadView = Expect<Equal<Parameters<typeof open.read>[0], CollectionView<Task>>>;

tasks.watch(open, (count) => {
  const value: Expect<Equal<typeof count, number>> = true;
  void value;
});

tasks.watchKeys(
  (keys, change) => {
    const list: Expect<Equal<typeof keys, readonly string[]>> = true;
    const changed: Expect<Equal<typeof change, KeysChange | undefined>> = true;
    void list;
    void changed;
  },
  { signal: new AbortController().signal },
);
