# Package

**Status:** draft for review. This page covers how the package is wired:
entry points, file locations, type-check configurations and test fixtures.
[Delivery](../contracts/delivery.md) owns every guarantee these implement; this
page only says where and how.

## Entry points

| `exports` entry | File | When |
| --- | --- | --- |
| `.` | `src/index.js` | Exists; gains the store exports with the store implementation |
| `./testing` | `src/testing/index.js` | Added in the change that implements test support, not before |
| `./elements` | `src/modules/elements/elements.js` | Exists: the element helpers |
| One entry per further DOM capability | Its module's entry file | Added with that capability |

Test support lives in `src/testing/`, outside `src/modules/`, because it is
package infrastructure for consumers' tests rather than a runtime capability.
It is planned now so the `exports` map and type checks account for it; it gets
no export until its code exists.

`files` in `package.json` already ships `src` and the README. Tests sit beside
modules as `*.test.js`, so the archive must exclude them; whether that is done
with `files` patterns or an ignore file is settled by the
[packed archive](#packed-archive) fixture.

## Type-check configurations

The single `tsconfig.json` today includes Node types and the DOM library for
every file and targets `ESNext`, which cannot enforce the compatibility
boundary. It is replaced by three configurations, which the `typecheck` script
runs one after another:

| Configuration | Root files | Library and types |
| --- | --- | --- |
| `tsconfig.pure.json` | Runtime files only: `src/index.js`, `src/modules/store/**/*.js` and `src/testing/**/*.js`, excluding `**/*.test.js`, `**/*.browser.test.js` and `**/*.bench.js` | `types: []`, an ECMAScript library both minimum runtimes implement, no DOM library, plus `types/shared-platform.d.ts` |
| `tsconfig.dom.json` | DOM capability runtime files, when they exist, with the same exclusions | `types: []`, the same ECMAScript library and the DOM library |
| `tsconfig.tooling.json` | Tests, benchmarks and configuration files | Node types, plus the DOM library for browser tests |

All three keep `allowJs`, `checkJs`, `strict` and `noEmit`.

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

The pure and DOM configurations use the newest ECMAScript library edition that
both Node 24.12 and Chromium 133 fully implement. ES2024 is the expected
candidate; it is confirmed against both runtimes before the configurations land.

## Tests and fixtures

| Fixture | Location | Purpose |
| --- | --- | --- |
| Node import | `tests/import.test.js` | Exists; extended to use every root export without browser globals |
| Tooling | `tests/tooling.test.js` | Exists; checks lint and type-check rejection of undocumented or mistyped code; gains the pure-program file listing check |
| Vitest browser tests | `*.browser.test.js` | Behavior in Chromium through Vitest; they load modules through Vitest's development server and say nothing about unchanged delivery |
| Static delivery | `tests/static/` | Plain HTML pages served unchanged, driven by Playwright |
| Packed archive | A test that packs the package and installs it into a temporary consumer | Added once the package has exports |
| Consumer types | Templates in `tests/types/`, run in the packed-archive consumer | Settles the `Selection` declaration and TypeScript consumer support |

### Static delivery

Vitest's browser mode serves modules through its own development server and
resolver, so a test that imports Lattice there does not show that an import map
or explicit URLs work with unchanged files. A separate fixture does:

- `tests/static/` holds plain HTML pages: one declaring an import map for
  `canonic-lattice` and importing the bare specifier, and one importing the
  explicit URL of the entry-point file.
- A Node test starts a minimal static file server that serves those pages and
  Lattice's `src/` files byte for byte, with no transformation, then drives
  Chromium through Playwright directly, not through Vitest's browser mode.
- Each page reports whether its import succeeded. The test also asserts that
  only entry-point and runtime files were requested and that each response body
  equals the file on disk.

### Packed archive

Once the package has exports, a test packs the package into a temporary
directory, creates a fresh consumer there with its own `package.json`, installs
the archive, and imports every entry point. The consumer is outside the
package, so it cannot reach Lattice's source tree, development dependencies or
configuration.

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
  lists, including a selection with a misspelled field. Expected failures are
  asserted by running the type checker and matching the specific error, as
  `tests/tooling.test.js` already does, not with suppression comments.

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

## Open questions

- Whether tests beside modules are excluded from the packed archive by `files`
  patterns or by an ignore file; the packed-archive fixture settles it.
