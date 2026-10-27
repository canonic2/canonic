# Package

**Status:** current implementation, with missing delivery fixtures identified
below. This page covers how the package is wired:
entry points, file locations, type-check configurations and test fixtures.
[Delivery](../contracts/delivery.md) owns every guarantee these implement; this
page only says where and how.

## Entry points

| `exports` entry                      | File                                   | When                                                       |
| ------------------------------------ | -------------------------------------- | ---------------------------------------------------------- |
| `.`                                  | `src/index.js`                         | Exists: `createStore`, `shallow` and the store's types     |
| `./collection`                       | `src/modules/collection/collection.js` | Exists: `createCollection` and its types; pure             |
| `./testing`                          | `src/testing/index.js`                 | Exists: record and collection selection declaration checks |
| `./elements`                         | `src/modules/elements/elements.js`     | Exists: the element helpers                                |
| One entry per further DOM capability | Its module's entry file                | Added with that capability                                 |

Test support lives in `src/testing/`, outside `src/modules/`, because it is
package infrastructure for consumers' tests rather than a runtime capability.

`files` in `package.json` ships `src` and the README, and excludes tests and
benchmarks beside modules with the negated patterns `!src/**/*.test.js` and
`!src/**/*.bench.js`. The [packed archive](#packed-archive) test checks that
the archive holds runtime source and no tests, fixtures or development files.

## Type-check configurations

Three configurations enforce the compatibility boundary; the `typecheck` script
runs them one after another:

| Configuration        | Root files                                                                                                                                                                                                         | Library and types                                                                  |
| -------------------- | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------------------------- |
| `tsconfig.pure.json` | Runtime files only: `src/index.js`, `src/report.js`, `src/modules/store/**/*.js`, `src/modules/collection/**/*.js` and `src/testing/**/*.js`, excluding `**/*.test.js`, `**/*.browser.test.js` and `**/*.bench.js` | `types: []`, the ES2024 library, no DOM library, plus `types/shared-platform.d.ts` |
| `tsconfig.dom.json`  | DOM capability runtime files, `src/modules/elements/**/*.js`, with the same exclusions                                                                                                                             | `types: []`, the ES2024 library and the DOM library                                |
| `tsconfig.json`      | Tests, benchmarks, examples and configuration files, and runtime files reached by their imports                                                                                                                    | Node types, plus the DOM library for browser tests                                 |

All three keep `allowJs`, `checkJs`, `strict` and `noEmit`. The tooling
configuration keeps the default name, `tsconfig.json`, so editors use it for
tests and configuration files.

### Keeping the pure program pure

`include` and `exclude` choose only the root files. A file imported from a root
file still enters the program, and so do the type packages that file pulls in.
Two rules keep the pure program free of test and Node declarations:

- Runtime modules never import a test, benchmark, fixture or development tool,
  as [delivery](../contracts/delivery.md#native-delivery-and-import-isolation)
  requires, so following imports from the root files reaches only runtime files.
- `types: []` stops automatic inclusion of every installed `@types` package.

Because a stray import would silently bring declarations back, a tooling test
lists the files in the pure program with the type checker's file-listing mode
and fails if anything is there besides the runtime files, the ECMAScript library
files and `types/shared-platform.d.ts`.

### Shared platform declarations

Without the DOM library or Node types, the type checker knows none of the APIs
pure modules may use: `AbortController`, `AbortSignal`, `queueMicrotask`,
`structuredClone` and `reportError`. A development-only declaration file,
`types/shared-platform.d.ts`, declares exactly those, so using anything else is
a type error. It is not shipped and declares nothing that only one runtime
provides; `reportError` is declared as possibly absent, matching the store's
fallback.

Because it is not shipped, public types must not depend on it. The
[consumer type fixture](#consumer-type-fixture) establishes that.

### ECMAScript edition

The pure and DOM configurations use ES2024, the newest ECMAScript edition both
Node 24.12 and Chromium 133 fully implement. Every ES2024 addition shipped in
Chromium 119 or earlier and in Node 22 or earlier, such as
`Promise.withResolvers`, `Object.groupBy`, resizable `ArrayBuffer` and the
regular expression `v` flag; ES2025 additions such as iterator helpers are
excluded.

## Tests and fixtures

| Fixture              | Location                                                                                   | Purpose                                                                                                                                                                                                                         |
| -------------------- | ------------------------------------------------------------------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Node import          | `tests/import.test.js`                                                                     | Exists; uses every root export and the collection entry point without browser globals                                                                                                                                           |
| Tooling              | `tests/tooling.test.js`                                                                    | Exists; checks lint and type-check rejection of undocumented or mistyped code, lists the pure program's files, checks that the pure configuration rejects DOM and Node globals, and that no runtime module imports test support |
| Vitest browser tests | `*.browser.test.js`                                                                        | Behavior in Chromium through Vitest; they load modules through Vitest's development server and say nothing about unchanged delivery                                                                                             |
| Static delivery      | Dedicated fixture not implemented                                                          | Required: plain HTML pages served unchanged, driven by Playwright                                                                                                                                                               |
| Packed archive       | `tests/package.test.js`                                                                    | Exists; packs the package, installs the archive into a temporary consumer, checks its contents and imports every entry point                                                                                                    |
| Consumer types       | Templates in `tests/types/`, run by `tests/package.test.js` in the packed-archive consumer | Covers selections, readers, checkers and callback inference under the documented consumer configuration                                                                                                                         |

### Static delivery

Vitest's browser mode serves modules through its own development server and
resolver, so a test that imports Lattice there does not show that an import map
or explicit URLs work with unchanged files. A dedicated Lattice delivery
fixture is still required:

- The fixture must hold plain HTML pages: one declaring an import map for
  `@canonic2/lattice` and importing the bare specifier, and one importing the
  explicit URL of the entry-point file.
- A Node test must start a minimal static file server that serves those pages and
  Lattice's `src/` files byte for byte, with no transformation, then drives
  Chromium through Playwright directly, not through Vitest's browser mode.
- Each page must report whether its import succeeded. The test must also assert that
  only entry-point and runtime files were requested and that each response body
  equals the file on disk. A separate DOM-subpath isolation case must observe
  the requested module graph.

`examples/serve.js` serves runnable examples and source unchanged through import
maps. That host is available for manual use; it is not the dedicated automated
fixture above. `tests/platform.browser.test.js` probes native browser lifecycle
through Vitest and does not establish static delivery or minimum-version support.

### Packed archive

`tests/package.test.js` packs the package with `npm pack` into a temporary
directory, creates a fresh consumer there with its own `package.json`, and
installs the archive by extracting it into the consumer's `node_modules`, which
is all an install does for a package with no dependencies. It then:

- lists the archive and checks that it holds the runtime source and no tests,
  type templates or platform declarations;
- runs a script in the consumer that imports every entry point, uses their
  exports, and checks that importing an internal path fails with
  `ERR_PACKAGE_PATH_NOT_EXPORTED`.

The consumer is outside the package, so it cannot reach Lattice's source tree,
development dependencies or configuration.

### Consumer type fixture

The files in `tests/types/` are templates, not a program checked in place:

- The packed-archive test copies them into the temporary consumer, with a
  minimal type-check configuration written for that consumer. It inherits
  nothing from Lattice: no shared configuration, no ambient declarations, no
  path aliases to Lattice's source.
- It runs twice, once with the DOM library and once with Node types and no DOM
  library, the two environments consumers have. It must pass without
  `types/shared-platform.d.ts`, which the archive does not contain.
- It covers the cases [delivery](../contracts/delivery.md#types-for-typescript-consumers)
  lists, including a selection with a misspelled field. Invalid fixture files
  assert a failing compiler exit and specific diagnostics. The valid reader
  fixtures also use narrow `@ts-expect-error` assertions for forbidden mutation;
  those assertions fail compilation when the forbidden operation becomes valid.
- `valid.ts` and the root `invalid-*.ts` fixtures cover the root entry point in
  both environments; `valid-collection.ts` and `invalid-collection-field.ts`
  cover the collection entry point in both environments; `valid-dom.ts` covers
  the element helpers and runs with the DOM library only, since that entry
  point is a DOM capability.
- `valid-collection.ts` exercises `CollectionReader`, traversal immutability,
  membership callback positions and both selection checkers. `valid.ts` covers
  `StoreReader` and detached read methods.
- The consumer configuration is the one consumers are told to use: `allowJs`
  with `maxNodeModuleJsDepth: 2`, as
  [delivery](../contracts/delivery.md#types-for-typescript-consumers) records.

## Integration evidence

The packed archive establishes that the package is complete and loadable as an
installed dependency. It does not establish delivery inside a particular
consumer: that consumer's packaging, server routes, documents and content
security policy. Each consumer plans and records that evidence in its own
specifications. Lattice requires of every such slice that it:

- imports the existing entry points, with no export added only for the test;
- covers every document that imports Lattice, each with its own import map or
  explicit URLs, since an import map does not apply to other documents such as
  iframes;
- runs from the consumer's installed artifact, without the Lattice source
  checkout or a link back into it.

## Benchmarks

No benchmark command exists. The change that adds the first `*.bench.js` adds a
benchmark project to `vitest.config.js` and a documented command to the package
README, as [performance and evidence](../contracts/performance.md#regression-protection)
requires.

## Decisions

- **Sequential type checks.** The `typecheck` script runs the three
  configurations one after another. Project references would add build-mode
  machinery that a no-emit check does not need.

- **Tests excluded by `files` patterns.** Negated patterns in `files` keep the
  exclusion beside the inclusion it narrows, in one place, instead of in a
  separate ignore file.
