/** @file Consumer type fixture: uses of the root entry point that must type-check. */
import { createStore, shallow } from '@canonic2/lattice';
import type { Selection, Store, StoreReader } from '@canonic2/lattice';

interface CounterState {
  count: number;
  label: string;
}

type IsAny<T> = 0 extends 1 & T ? true : false;
type Equal<A, B> =
  (<T>() => T extends A ? 1 : 2) extends <T>() => T extends B ? 1 : 2 ? true : false;
type Expect<T extends true> = T;

const store = createStore({ count: 0, label: 'Acme' });

export type Imports = [
  Expect<Equal<IsAny<typeof createStore>, false>>,
  Expect<Equal<IsAny<typeof shallow>, false>>,
  Expect<Equal<IsAny<typeof store>, false>>,
  Expect<Equal<IsAny<Selection<CounterState, string>>, false>>,
  Expect<Equal<IsAny<StoreReader<CounterState>>, false>>,
];

// Generic inference: the store is typed by the state passed to it.
const typed: Store<CounterState> = store;
const reader: StoreReader<CounterState> = { get: store.get, watch: store.watch };
const detachedGet = reader.get;
detachedGet();
// @ts-expect-error A consumer cannot mutate through a reader.
reader.update({ count: 1 });
// @ts-expect-error Reader methods cannot be replaced.
reader.get = store.get;
const snapshot = typed.get();
export type Snapshot = Expect<Equal<typeof snapshot, Readonly<{ count: number; label: string }>>>;

store.update({ count: 1 });
store.update((state) => ({ label: `${state.label}!` }));
store.batch(() => {
  store.update({ count: 2 });
});

// A selection checked by `satisfies`: read receives the state type.
const label = {
  fields: ['count'],
  read: (state) => `${state.count} clicks`,
} satisfies Selection<CounterState, string>;
export type ReadState = Expect<Equal<Parameters<typeof label.read>[0], Readonly<CounterState>>>;

// The value run receives matches read's result.
store.watch(label, (value, previous) => {
  const current: Expect<Equal<typeof value, string>> = true;
  const before: Expect<Equal<typeof previous, string | undefined>> = true;
  void current;
  void before;
});

store.watch(
  { fields: ['count', 'label'], read: (state) => [state.count, state.label], equals: shallow },
  () => {},
  {
    signal: new AbortController().signal,
  },
);
