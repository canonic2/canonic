# Delivery and public API

## Purpose and status

This contract defines what Lattice offers to the applications that use it: its
entry points, what counts as public, the rules every export follows, the
platforms it supports, how applications load it with and without a bundler,
how TypeScript consumers get its types, and how its versions change. This topic
owns runtime delivery, compatibility and public promises. The
[README](../../../README.md) owns setup and commands.

**Status:** agreed contract. The package is unpublished and private;
`package.json` owns its current version. The root exports the store core,
`@canonic2/lattice/collection` the keyed collection,
`@canonic2/lattice/elements` the element helpers, and
`@canonic2/lattice/testing` the record and collection selection checks.
The Node import, packed-consumer, type, testing-separation and runner-independent
support cases have tests. Dedicated unchanged-source and DOM-subpath isolation
fixtures remain gaps in [acceptance](#acceptance).

## Native delivery and import isolation

Runtime modules are modern JavaScript `.js` files with native ES modules and
`type: module`. They are served unchanged, with no compilation, production
bundler, transpilation, generated JavaScript copy or runtime dependencies.
CommonJS requires an explicit, documented host compatibility need; it is not
a default or a parallel distribution. Development tooling may use a compiler
or server without changing runtime delivery.

Runtime exports stay statically analyzable. They are not hidden behind CommonJS
bridges, dynamic export tables or runtime module registries. `sideEffects: false`
is a correctness promise: evaluating any runtime module must not register
elements, touch the DOM, install global listeners, mutate host globals, start
background work or perform I/O. Initialization happens through explicit calls;
required initialization must not be a side-effect-only import a bundler could
discard. Importing one capability must not initialize another.

Keep import graphs narrow, bounded and acyclic, with pure state independent of
DOM integrations and optional capabilities. Internal imports do not go through
an aggregate entry point that pulls in unrelated capabilities. Neither ESM nor
`sideEffects: false` alone proves unused code is absent from a consumer bundle.

## Entry points

Each entry point is a subpath in the `exports` map of `package.json`, pointing
at an unchanged source file:

| Specifier                      | Exports                                                               | May load                                                                                     |
| ------------------------------ | --------------------------------------------------------------------- | -------------------------------------------------------------------------------------------- |
| `@canonic2/lattice`            | The store core: `createStore`, `shallow` and their types              | Pure modules only: no DOM, no browser globals                                                |
| `@canonic2/lattice/collection` | The keyed collection: `createCollection` and its types                | Pure modules only: its own module and the shared callback error handling, not the store core |
| `@canonic2/lattice/testing`    | Test support, such as the selection declaration check                 | Platform APIs and Lattice's own modules only; never imported by a runtime module             |
| `@canonic2/lattice/elements`   | `captureInputs`, `replayInputs`, `skipUpgradeAttribute`, `defineOnce` | Its own module, with no imports or DOM access during evaluation                              |

- The root entry point stays pure. It imports in Node without browser globals,
  and importing it in a browser loads no DOM code.
- Each DOM capability gets its own subpath when it is implemented, so an
  application that does not use it never loads it. Browsers load every module an
  import reaches, so separate entry points, not tree shaking, keep native
  delivery small.
- There is no aggregate entry point that re-exports every capability, and no
  wildcard export of internal files.
- A subpath is added in the same change as the capability it exports, with its
  JSDoc and tests. An entry point never exists ahead of its capability.
- Test support is independent of any test runner. It imports no test framework
  and none of Lattice's development dependencies, and it reports a failed check
  by throwing an `Error` with a descriptive message, which every runner treats
  as a failure. A consumer needs nothing beyond Lattice to use it.

## What is public

- The public API is exactly what the entry points export, and the documented
  behavior of those exports: their parameters, return values, events, timing,
  ordering, errors and cleanup obligations as their JSDoc and specs describe.
- Files under `src/`, their internal exports and their paths are not public.
  Consumers import only through entry points.
- Error types are public; error messages are not. A consumer may branch on an
  error's type, such as the `TypeError` a rejected write throws, never on
  message text.
- Plain objects that consumers create and Lattice reads, such as a selection's
  `fields`, `read` and `equals`, are public shapes documented with `@typedef`.
  The `Selection<State, Value>` type is exported for consumers to annotate their
  selections.

## Export rules

- Named exports only. No default exports, so every import names what it uses
  and renames are visible.
- Factories and functions, not singletons. An export never holds application
  state at module level; each call to a factory creates independent state.
- Names say what something is or does: `createStore`, `shallow`. A factory is
  named `create<Thing>`; a registration function is named `define<Element>`.
- Every export has complete JSDoc written for a consumer who reads only the
  documentation: types, parameters, returns, timing, equality, errors,
  cancellation and cleanup where applicable. Static checks do not establish
  nested immutability or runtime validity; behavioral tests cover those promises.
- An export that a consumer cannot use without another export comes from the
  same entry point, so one import is enough for one capability.

## Platform support

Lattice supports runtimes from a stated minimum version and ships no polyfills or
fallbacks below it:

| Runtime  | Minimum | Why                                                                                                                                                  |
| -------- | ------- | ---------------------------------------------------------------------------------------------------------------------------------------------------- |
| Node     | 24.12   | The pure entry points and the package's tooling                                                                                                      |
| Chromium | 133     | The oldest version providing every feature a specified capability relies on; `Element.moveBefore` and `connectedMoveCallback` are the newest of them |

- A host that embeds Chromium, such as an Electron application or an editor's
  webview, is supported when its embedded Chromium meets the minimum. Electron
  ships its own Chromium on its own schedule, so a host qualifies by the version
  it actually embeds, not by being current.
- A capability may rely only on platform features available in the minimum
  version, such as `AbortSignal.any`, adopted stylesheets, `Element.moveBefore`
  and `connectedMoveCallback`. Its spec names the features it depends on. A
  capability that needs a newer feature raises the minimum, which is a contract
  change.
- Other browser engines are not supported until a consuming application needs
  them. Adding one is a contract change that lists the features to verify.

### Compatibility boundary

Modern syntax and APIs must be available in every runtime an entry point
supports at the minimum versions above. No feature detection or polyfill substitutes for meeting this boundary.
The documented optional `reportError` fallback is shared error handling for
supported hosts, not a compatibility layer below the minimum runtime.

- Pure modules reachable from the root and collection entry points and test
  support run in both Node and Chromium. They use only ECMAScript built-ins
  both minimum versions implement, plus shared APIs such as `AbortController`,
  `queueMicrotask` and `structuredClone`. They do not use runtime-specific
  globals such as `window`, `document`, `customElements`, `process` or
  `Buffer`.
- DOM modules reachable only from DOM subpaths may use the minimum Chromium's
  APIs and no Node API.
- Pure-module type checking excludes DOM and Node global types and targets an
  ECMAScript edition both minimum runtimes implement. Shared platform API types
  must not admit unrelated runtime-specific globals. DOM modules are checked
  with DOM types and without Node types.
- Type correctness does not establish minimum-runtime API availability.
  Compatibility evidence combines Node import tests of pure entry points,
  browser tests and each capability's platform features checked against the
  minimum versions. Raising a minimum is a delivery contract change.

### Verification evidence

Two kinds of evidence are kept apart:

| Evidence                                                                                               | Establishes                                                      | Does not establish                                  |
| ------------------------------------------------------------------------------------------------------ | ---------------------------------------------------------------- | --------------------------------------------------- |
| Package tests: Node tests and Chromium browser tests through Playwright                                | Behavior in Node and in the Chromium version Playwright provides | Behavior in any particular Electron or webview host |
| Host evidence: integration results recorded in research with the host and embedded Chromium conditions | Support in that host version                                     | Support in other hosts or versions                  |

A host is described as verified only when a linked research or consumer
integration record establishes that scope. Specs state support requirements
and coverage gaps; they do not keep dated check-run logs. Meeting the
minimum-version rule alone does not establish host verification.

## Consumption

### Without a bundler

An application that serves its UI files unchanged serves Lattice's `src/` files
unchanged too:

- Lattice's own imports are relative and include the `.js` extension, so served
  files resolve each other without rewriting.
- A browser resolves a bare specifier such as `@canonic2/lattice` only through an
  import map; without one, resolution fails before any request reaches the
  server. An application that imports Lattice by its package name therefore
  declares an import map that maps each specifier it uses to the URL of the
  served entry-point file, following the `exports` map.
- An application without an import map imports the explicit URL of a public
  entry-point file instead, such as the served path of `src/index.js`. Only the
  files the `exports` map names may be imported this way, never internal files.
- The application's server serves the resolved URLs; it does not resolve bare
  specifiers.
- Serving needs no build step, no transpilation and no change to the files.

### With a bundler

Bundlers resolve the `exports` map and may remove unused exports, aided by
`sideEffects: false`. A claim that a bundler omits an unused capability needs
a consumer bundle fixture showing that unused code is omitted and used code
still behaves correctly. Import isolation is also exercised through Node and
browser fixtures; a bundle fixture adds no production build to Lattice.

### Installation

While unpublished, applications depend on Lattice through a local path
dependency in their own `package.json`, such as `link:` with pnpm, and keep their
own lockfile. Lattice has no runtime dependencies, so it adds nothing else to a
consumer's dependency tree.

A linked dependency resolves into Lattice's own folder, where its development
dependencies and every source file are present, so it can hide a file missing
from `files` or a reliance on the development environment. The isolated consumer
fixture packs the package, installs the archive into a fresh fixture outside
the package, and imports every entry point there.

## Types for TypeScript consumers

Lattice's types live in its JSDoc and are checked by TypeScript inside the
package. Consumer types are exercised separately because TypeScript does not
infer types from JavaScript dependencies by default. The packed consumer
fixture must establish:

- A fixture outside the package, with its own TypeScript configuration, imports
  Lattice through its public package name.
- Generic inference survives the import: the value returned by `createStore` is
  typed by the state passed to it, and a selection's `read` receives that state
  type.
- An invalid call, such as writing a key that is not in the state or passing a
  `read` result of the wrong type to `run`, is a type error, and no imported
  value is typed `any`.
- A selection annotated with `@satisfies {Selection<State, Value>}` infers
  correctly when valid; a misspelled field is a type error; `read` receives the
  state type; and the value `run` receives matches `read`'s result.

Lattice ships JavaScript with JSDoc and no declaration files. A TypeScript
consumer sets `allowJs: true` and
`maxNodeModuleJsDepth: 2` in its own configuration; the
[package README](../../../README.md) documents this. With the default depth of 0
every import is untyped, and with depth 1 TypeScript reads an entry-point file
but not the modules it re-exports, so the store's exports can arrive as `any`.
The packed-archive consumer runs the fixture with the documented settings,
in a DOM environment and a Node environment, so a change that needs a deeper
setting fails there.

### Agreed direction: generated declarations

The owner has requested declaration generation as stage 5 of the
[supported-workload roadmap](../roadmap.md). This is an agreed implementation
direction, not an available package feature. Current consumers still need the
settings above until that stage is implemented and accepted.

Package preparation will generate `.d.ts` files from the authored JSDoc, with
no handwritten declarations, runtime transformation or runtime dependencies.
Public exports and the packed archive will include the generated declarations.
Isolated Node and browser TypeScript consumers must use normal configurations,
without `allowJs` or `maxNodeModuleJsDepth`, and exercise every entry point,
generic inference and invalid calls. A deterministic generation check must
detect stale declarations. Implementation and consumer acceptance are required;
this stage does not need a performance lab. The roadmap owns sequencing and
the review gates for earlier proposed APIs.

## Versions

- While Lattice is unpublished, its API may change in any release. Consumers
  update together with it.
- After publication, versions follow semantic versioning: a breaking change to
  anything public raises the major version, a new export or option the minor
  version, and a fix the patch version. Before 1.0.0, a breaking change raises
  the minor version.
- Public behavior described in the specs is part of the API. Changing a
  documented guarantee, such as notification order or cleanup timing, is a
  breaking change even when the function signatures stay the same.
- Removing or renaming an export is a breaking change. There are no deprecation
  aliases while Lattice is unpublished.
- Publication, its registry and its name are decided separately; the package
  stays `private` until then.

## Acceptance

| Requirement                     | Evidence                                                                                                                                                          |
| ------------------------------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| Pure root entry point           | A Node test imports `@canonic2/lattice` without browser globals and uses every export                                                                             |
| Pure collection entry point     | A Node test imports `@canonic2/lattice/collection` without browser globals and uses its export                                                                    |
| Separate DOM subpaths           | Required fixture gap: observe that importing a DOM subpath loads no unrelated capability modules                                                                  |
| Unchanged delivery              | Required fixture gap: exercise unchanged source through an import map and explicit entry-point URLs; the example host alone does not establish automated coverage |
| No internal imports             | The `exports` map lists only entry points, and a review confirms consumers import nothing else                                                                    |
| Testing stays separate          | No module reachable from a runtime entry point imports `@canonic2/lattice/testing`                                                                                |
| Runner-independent test support | `@canonic2/lattice/testing` imports no test framework or development dependency, and works from the isolated consumer fixture                                     |
| Packaged artifact               | The isolated consumer check imports every entry point from the packed archive                                                                                     |
| TypeScript consumers            | `tests/package.test.js` checks valid and invalid fixtures in an isolated packed consumer, including readers and selection checkers                                |
| Platform features               | Each capability's spec lists the platform features it uses; none is newer than the minimum Chromium version                                                       |
| Hosts                           | Link research or consumer integration evidence for any verified-host claim; no such claim follows from package tests alone                                        |

## Decisions

- **Separate entry points over one barrel.** Native browser delivery has no tree
  shaking, so a single entry point re-exporting everything would load every
  capability for every consumer.
- **A minimum version, no polyfills.** A stated minimum lets each host check
  its embedded Chromium against one number, and keeps capabilities from using
  features some supported host lacks. Polyfills and fallbacks below it would add
  code and test paths no consumer needs.
- **Import maps or explicit URLs, not server-side resolution.** Bare specifiers
  are resolved in the browser before any request, so a server cannot map them.
- **Generate declarations from JSDoc.** Agreed direction for roadmap stage 5:
  normal TypeScript consumption without changing authored JavaScript or adding
  runtime cost. Generation and isolated consumer acceptance remain to implement.
- **Named exports only.** Explicit names keep imports searchable and make
  renames visible to every consumer.
- **The package name is `@canonic2/lattice`** (2026-10-24), in the `@canonic2`
  scope Canonic's other npm packages use. It applies now, while the package is
  private; publication itself is decided separately.

## Open questions

- Which hosts, and which of their versions, get recorded host evidence first.
