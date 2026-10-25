# Page index

Status: implemented for preview and docs discovery in the native server,
including its resident index and definition watcher. Compilation and
pre-warming are separate responsibilities of the native compiler.

## Purpose and ownership

The canvas needs a complete, ordered page list without waiting for the preview
compiler to start. Listing must preserve the definition behavior in
[TypeScript previews](../pages/previews.md) and [docs](../pages/docs-pages.md), including
validation, errors, IDs, scenarios and sidebar order.

This topic owns discovery, index reuse, evaluation, failure recovery and saved
index bounds. [Performance](../system/performance.md#measured-interactions) owns
the agreed complete-list target; [listing research](../../research/page-listing.md)
records measurements and their limits. [Compiler](compiler.md) owns building the
pages after they are listed. A fast index alone does not establish that the
first page meets its render target.

## Requirements

### Discovery and compatibility

- Listing returns the same `{ previews, docs, errors }` answer as the Node
  definition evaluator's index ([compiler.cjs](../../server/node/preview/compiler.cjs)). Preview and docs IDs share one namespace. The first
  definition in discovery order wins; later duplicates produce a
  file-prefixed `Duplicate preview id` problem.
- Default patterns are `**/*.workbench.ts` and `**/*.workbench.tsx`.
  `previews.include` replaces them. Skip dot-prefixed entries, symlinks,
  `node_modules`, `dist`, `build` and `coverage`, and what the project's
  `.gitignore` files ignore: each regular `.gitignore` applies to its own
  folder, the deepest one with a matching rule decides, and a negation cannot
  bring back a path inside an ignored folder, as in Git. Global and
  `.git/info/exclude` files do not apply, so the list does not depend on the
  machine. Both the native index and the definition evaluator apply the rules, and the
  saved index records the `.gitignore` files it applied, so an edit to one
  invalidates the saved answer. With natively matched
  patterns, the walk starts at each pattern's literal leading folders
  (`demo/**/*.workbench.ts` walks `demo/`), unless the skip rule would have
  kept the full walk out of them; the files found are the same.
- Ordering is a depth-first walk with each directory's children sorted by
  JavaScript `localeCompare`. It includes mixed case, punctuation, digits and
  non-ASCII names; sorting complete paths by Rust byte order is insufficient.
- Plain path/globstar patterns use native matching. Patterns outside that
  subset use the definition evaluator's picomatch selection, so
  braces, negation and other supported patterns retain their meaning.
- Loading or validating one definition can fail without hiding valid pages.
  Error messages and their order follow the definition evaluator's listing
  contract.

### Parsing and evaluation

- Literal-only `definePreview` definitions are parsed without running project
  code. Accepted values include strings, finite numbers, booleans, null,
  arrays, objects, negative numbers, expression-free template strings and
  erased TypeScript wrappers.
- Parsing is conservative. Runtime imports other than the authoring API, executable
  statements, computed properties, spreads, getters, functions and values
  whose JavaScript semantics cannot be preserved require evaluation.
- Computed definitions and all `defineDocs` definitions are evaluated in one
  batch with the definition evaluator's loader, validation and projection. Project
  configuration and the files read by evaluation participate in invalidation.
- Rust validates and projects parsed literals; the definition evaluator owns
  evaluated definitions. Behavioral parity tests hold the two implementations to the
  same answer. A parse failure requests evaluation rather than becoming a
  Rust-only syntax error.
- Untrusted workspaces list no definitions. The server checks trust before
  indexing; it also applies `previews: false`. Allowing literal-only listing
  in an untrusted workspace requires a separate product decision.

### Saved state and invalidation

- The index records definition stamps and the stamps of their relevant
  inputs: source paths checked for existence and files read during evaluation.
  Stamps use modification time and size. Unchanged records are reused without
  reading or evaluating their definitions.
- Directory stamps allow reuse of known children. Changed directories are
  read again, so additions, removals and renames appear on the next listing.
  With no valid saved index, discovery walks the project. A full-walk override
  is available for diagnostics.
- The binary, the definition evaluator's scripts (`evaluate-definitions.cjs`
  and `compiler.cjs`), the authoring API and the index options form the cache
  identity. A changed identity or saved format rebuilds the index. A failed
  evaluation retries because its dependency graph may be incomplete.
- The server keeps the last ordered answer with its generation; the index
  answers `reuse_answer` only for that generation, and anything else lists
  the records again.
- Cache reads and writes are optional optimizations. Unreadable or corrupt
  data cannot prevent listing. Publish each save through a temporary file and
  atomic rename; report save failures without discarding a successful answer.

### Process and resource ownership

The Rust package in `server/` has a library and a thin binary entry
point. `workbench-server index` consumes one JSON request on stdin and writes
one JSON response, then exits. The protocol is documented in
[the server README](../../server/README.md#index-protocol).
The [native ownership contract](../system/modules.md#rust-ownership) separates
pure definition rules, application stages, shared observations and adapters.

The server's [catalog](../../server/src/catalog.rs) owns the request: the
saved index's location, its identity, ordering and duplicate IDs. The resident
index answers it in the server process. Evaluation uses a second
request, reusing the first one's parsed records, and runs in the server's
[definition-index evaluator](../system/server.md#the-definition-index-evaluator),
which owns its process, runtime preparation and deadline. `workbench-server index` runs
one transaction per process, and `workbench-server list` prints the read
answer.

| Resource | Bound and release rule |
| --- | --- |
| Saved records | At most 32 MiB of versioned postcard data per project; a larger index is not saved, and the timings report it as a save error |
| Project saves | At most 32 root-digest directories under `canonic-workbench-index/` in the system temporary folder; evict inactive directories after seven days and oldest directories beyond the count bound during index requests |
| Evaluator run | The request's deadline; the evaluator is killed at it ([definition-index evaluator](../system/server.md#the-definition-index-evaluator)) |
| Index answer | At most 128 MiB, the one-process index's output bound and the resident index's answer bound |

Saved indexes belong outside the project. They are not authoring files or
portable build outputs. A failing or timed-out index adds a
`Workbench previews: …` problem to the page tree. Listing does not require
preview compilation; which listings start an evaluator process is the
[server's process model](../system/server.md#processes-and-connections). The page tree then configures the compilers,
which build and pre-warm what it lists ([server](../system/server.md#previews)).

### Distribution

Every supported VSIX target contains its matching native executable. Packaging
builds locked release dependencies for that Rust target or uses an explicitly
provided prebuilt executable. Missing target/linker prerequisites stop
packaging. Ship the executable, excluding Rust source, Cargo caches and test
reports. [Code quality](../system/code-quality.md) owns the build and verification
policy; [the native README](../../server/README.md#checks-and-packaging) owns
developer commands.

## Acceptance and evidence

These criteria describe observable behavior. Existing automated evidence is
named below; a test's presence does not establish performance or platform
acceptance.

| ID | Acceptance criterion | Existing evidence |
| --- | --- | --- |
| PI-01 | Native and definition-evaluator answers agree for literal/computed previews, docs, invalid definitions, duplicate IDs, custom globs and discovery order | Parity tests in [preview.test.js](../../server/node/preview.test.js) against `workbench-server list`, and [answer tests](../../server/src/catalog/answer.rs) |
| PI-02 | Literal listing requires no JavaScript executable; executable definitions use evaluation | [Server integration tests](../../server/node/server.test.js); [literal unit and property tests](../../server/src/literal.rs) |
| PI-03 | Refresh observes additions, removals, renames, retitles and changed imported list fields | [Native CLI integration](../../server/tests/integration/main.rs) and [server.test.js](../../server/node/server.test.js)'s added-definition test |
| PI-04 | Invalid saves rebuild, only matching generations reuse the complete answer, and failed saves preserve successful listing | [Saved-index unit test](../../server/src/saved.rs), native CLI generation/handoff/save-failure tests |
| PI-05 | A failing or timed-out index is a page-tree problem, and the rest of the configuration still answers | [Native CLI deadline test](../../server/tests/integration/main.rs); the page-tree problem has no dedicated test |
| PI-06 | Trust and disabled-preview controls prevent project evaluation | [Preview server integration tests](../../server/node/preview.test.js) |
| PI-07 | Cache eviction, byte bounds and process cleanup follow the resource table | Implemented bounds; dedicated boundary/eviction and descendant-cleanup assertions remain verification gaps |
| PI-08 | Installed platform binary lists pages with the same public manifest shape | Host packaging/install probe recorded in [listing evidence](../../research/page-listing.md); acceptance on all six targets is not established by that probe |

## Remaining work

The implemented design uses directory-stamp reuse, literal parsing and the
definition evaluator, a resident index and a definition watcher. [Listing research](../../research/page-listing.md) records measured
results and target gaps; [comparison evidence](../../research/instant-first-page.md#stage-1-2026-10-15)
supports the architecture without defining the shipping interface.

- Studio has no native-library integration contract. The presence of a Rust
  library does not establish an embeddable indexing API.
- Metadata stamps do not establish content identity when both modification
  time and size are preserved. Stronger invalidation would need a separate
  design and measurement.
