# TypeScript and JSDoc

Lattice ships JavaScript with JSDoc types. Its runtime source is the same whether
you consume it from JavaScript or TypeScript.

## Configure a TypeScript consumer

Include these settings in your project's TypeScript configuration:

```json
{
  "compilerOptions": {
    "allowJs": true,
    "maxNodeModuleJsDepth": 2
  }
}
```

The depth lets TypeScript read the modules behind Lattice's entry points.
Without that configuration, the exports can arrive as untyped JavaScript.
Lattice does not ship declaration files.

## Type a selection

A `satisfies` annotation checks fields and supplies the reader's state type:

```ts
import { createStore } from '@canonic2/lattice';
import type { Selection } from '@canonic2/lattice';

interface Task {
  text: string;
  done: boolean;
}

const task = createStore({ text: 'Write the guide', done: false });

const status = {
  fields: ['done'],
  read: (state) => (state.done ? 'Complete' : 'In progress'),
} satisfies Selection<Task, string>;

task.watch(status, (value, previous) => {
  // value: string; previous: string | undefined
});
```

A misspelled field or the wrong reader result becomes a type error.

In JavaScript, use the equivalent JSDoc annotation:

```js
/** @typedef {{ text: string, done: boolean }} Task */

/** @satisfies {import('@canonic2/lattice').Selection<Task, string>} */
const status = {
  fields: ['done'],
  read: (state) => (state.done ? 'Complete' : 'In progress'),
};
```

Use `ItemSelection<Task, Value>` for one collection item and
`CollectionSelection<Task, Value>` for an aggregate. Both come from
`@canonic2/lattice/collection`.

## Expose reader interfaces

A capability can describe its public state with `StoreReader`:

```ts
import type { StoreReader } from '@canonic2/lattice';

const state: StoreReader<Task> = {
  get: task.get,
  watch: task.watch,
};
```

Consumers can read and watch, but the type exposes no mutation methods.

For a collection, forward `size` with a getter:

```ts
import { createCollection } from '@canonic2/lattice/collection';
import type { CollectionReader } from '@canonic2/lattice/collection';

const tasks = createCollection([['guide', { text: 'Write the guide', done: false }]]);

const items: CollectionReader<Task> = {
  keys: tasks.keys,
  get: tasks.get,
  has: tasks.has,
  get size() {
    return tasks.size;
  },
  watchKeys: tasks.watchKeys,
  watchItem: tasks.watchItem,
  watch: tasks.watch,
};
```

Copying `size: tasks.size` would capture one number instead of keeping it current.

Reader types restrict typed access. Constructing the wrapper object also leaves
mutation methods out at runtime. Merely annotating a full writable owner as a
reader does not remove its runtime methods.

A selection's `CollectionView<Task>` is narrower: it exposes traversal without
watchers. The [API overview](../api/README.md) lists the types by entry point.
