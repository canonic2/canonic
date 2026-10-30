# Lattice

Lattice's implemented package provides native JavaScript primitives for state
and Web Component bindings, with no runtime dependencies. It is unpublished.
Other language implementations, native platforms and shared code generation
are not available yet.

Supported runtimes are **Node 24.12+** for pure entry points and tooling, and
**Chromium 133+** for browser use. Embedded hosts qualify by their Chromium
version. Other browser engines are unsupported; Lattice ships no polyfills.

The [guide](docs/README.md) starts with a working browser example and teaches
state, selections, components and lists step by step. The
[API reference](docs/api/README.md) collects signatures, options and public types.

| Entry point                    | Exports                                                                                                                                                                                                                                                                                                    |
| ------------------------------ | ---------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `@canonic2/lattice`            | The store core: `createStore`, `shallow`, and the `Selection`, `Store`, `StoreReader`, `StoreOptions` and `WatchOptions` types. Pure: it loads no DOM code and runs in Node                                                                                                                                |
| `@canonic2/lattice/collection` | The keyed collection: `createCollection`, and the `Collection`, `CollectionReader`, `CollectionView`, `ItemSelection`, `CollectionSelection`, `AggregateSelection`, `MembershipChanges`, `KeysChange`, `CollectionOptions` and `CollectionWatchOptions` types. Pure: it loads no DOM code and runs in Node |
| `@canonic2/lattice/elements`   | The element helpers: `captureInputs`, `replayInputs`, `skipUpgradeAttribute` and `defineOnce`                                                                                                                                                                                                              |
| `@canonic2/lattice/testing`    | Test support: `checkSelection` for store/item selections and `checkCollectionSelection` for aggregates. Both check declared field dependencies. For tests only; no runtime module imports them                                                                                                             |

Source files run unchanged in supported runtimes. There is no production build,
bundler, transpilation or CommonJS entry point. TypeScript checks JavaScript and
JSDoc with `noEmit`; it does not produce the shipped code.

Collections support atomic bulk membership through `updateMembership` and
reversible incremental summaries through `watchAggregate`. See
[collections](docs/guide/collections.md) and [counts and sums](docs/guide/aggregates.md)
for when to use each. Ordinary insert/remove still construct their own frozen
keys arrays; batching combines delivery rather than those individual copies.

## Using Lattice from TypeScript

Lattice ships JavaScript with JSDoc types and no declaration files. A
TypeScript consumer reads those types by adding these settings to its own
configuration:

```json
{
  "compilerOptions": {
    "allowJs": true,
    "maxNodeModuleJsDepth": 2
  }
}
```

Without them, TypeScript treats Lattice's exports as untyped. Annotate a
selection with `satisfies Selection<State, Value>` to check its fields and type
its `read`.

## Development

Use Node 24.12 or newer and pnpm 10.33.0. From the repository root:

```sh
pnpm --dir packages/lattice install --frozen-lockfile
pnpm --dir packages/lattice run browser:install
```

Run categories independently:

| Category                                                                   | Command from the repository root                     |
| -------------------------------------------------------------------------- | ---------------------------------------------------- |
| Lint, including JSDoc                                                      | `pnpm --dir packages/lattice run lint`               |
| Apply safe lint fixes                                                      | `pnpm --dir packages/lattice run lint:fix`           |
| Apply formatting                                                           | `pnpm --dir packages/lattice run format`             |
| Check formatting                                                           | `pnpm --dir packages/lattice run format:check`       |
| Check JavaScript/JSDoc types: pure, DOM and tooling configurations in turn | `pnpm --dir packages/lattice run typecheck`          |
| Node unit tests                                                            | `pnpm --dir packages/lattice test`                   |
| Watch unit tests                                                           | `pnpm --dir packages/lattice run test:watch`         |
| Chromium tests through Playwright                                          | `pnpm --dir packages/lattice run test:browser`       |
| Watch browser tests                                                        | `pnpm --dir packages/lattice run test:browser:watch` |

Append a test path to the relevant test command for focused verification.
The unit tests include `tests/package.test.js`, which packs the package with
`npm pack` and type-checks consumer fixtures; it takes about a minute. Unit
tests run with `--expose-gc` so retention tests can force garbage collection.
Browser tests use a real browser, not a DOM emulator. Vitest's development server
is test infrastructure; it is not a production build requirement.

Oxlint runs native JSDoc rules and the `require-jsdoc` rule from
`eslint-plugin-jsdoc` through its JavaScript plugin API. No ESLint CLI runs.
Oxfmt formats source, configuration and documentation. Dependencies are pinned
in `package.json` and `pnpm-lock.yaml`.

Oxlint's JavaScript plugin API is currently alpha. Tooling fixtures verify the
pinned combination against missing module/function/private-method documentation,
incorrect parameter names and a JSDoc type mismatch. TypeScript 6.0.3 is pinned
to satisfy the JSDoc plugin's current TypeScript compatibility range.

Contributor language, JSDoc and tooling instructions are supplied directly by
the package's agent rules.
