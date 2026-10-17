# Page index

Status: implemented for server-side preview and docs discovery. The Rust
compiler host is separate lab work; this index does not change compilation,
pre-warming or definition watching.

## Purpose and ownership

The canvas needs a complete, ordered page list without waiting for the preview
compiler to start. Listing must preserve the definition behavior in
[TypeScript previews](../previews.md) and [docs](../docs-pages.md), including
validation, errors, IDs, states and sidebar order.

This topic owns discovery, index reuse, evaluation, failure recovery and saved
index bounds. [Performance](performance.md#page-listing) owns measured timings
and the agreed complete-list target. [Compiler](compiler.md) owns building the
pages after they are listed. A fast index alone does not establish that the
first page meets its render target.

## Requirements

### Discovery and compatibility

- Listing returns the same `{ previews, docs, errors }` answer as the
  compiler's index. Preview and docs IDs share one namespace. The first
  definition in discovery order wins; later duplicates produce a
  file-prefixed `Duplicate preview id` problem.
- Default patterns are `**/*.workbench.ts` and `**/*.workbench.tsx`.
  `previews.include` replaces them. Skip dot-prefixed entries, symlinks,
  `node_modules`, `dist`, `build` and `coverage`.
- Ordering is a depth-first walk with each directory's children sorted by
  JavaScript `localeCompare`. It includes mixed case, punctuation, digits and
  non-ASCII names; sorting complete paths by Rust byte order is insufficient.
- Plain path/globstar patterns use native matching. Patterns outside that
  subset use the compiler's picomatch selection through the evaluator, so
  braces, negation and other supported patterns retain their meaning.
- Loading or validating one definition can fail without hiding valid pages.
  Error messages and their order follow the compiler's listing contract.

### Parsing and evaluation

- Literal-only `definePreview` definitions are parsed without running project
  code. Accepted values include strings, finite numbers, booleans, null,
  arrays, objects, negative numbers, expression-free template strings and
  erased TypeScript wrappers.
- Parsing is conservative. Runtime imports other than the authoring API, executable
  statements, computed properties, spreads, getters, functions and values
  whose JavaScript semantics cannot be preserved require evaluation.
- Computed definitions and all `defineDocs` definitions are evaluated in one
  batch with the compiler's loader, validation and projection. Project
  configuration and the files read by evaluation participate in invalidation.
- Rust validates and projects parsed literals; the compiler owns evaluated
  definitions. Behavioral parity tests hold the two implementations to the
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
- Binary, evaluator, compiler, authoring API and index options form the cache
  identity. A changed identity or saved format rebuilds the index. A failed
  evaluation retries because its dependency graph may be incomplete.
- A complete ordered answer is saved beside the native records. Its identity
  and generation must agree with the native index before it can be reused.
  A missing or corrupt answer is rebuilt from the records.
- Cache reads and writes are optional optimizations. Unreadable or corrupt
  data cannot prevent listing. Publish each save through a temporary file and
  atomic rename; report save failures without discarding a successful answer.

### Process and resource ownership

The stable Rust package in `server/` has a library and a thin binary entry
point. `workbench-server index` consumes one JSON request on stdin and writes
one JSON response, then exits. The protocol is documented in
[the native README](../../server/README.md#protocol).
The [native ownership contract](../modules.md#rust-ownership) separates
pure definition rules, application stages, shared observations and adapters.

The TypeScript adapter in `server/node/src/modules/page-index/` owns binary selection,
temporary storage, process lifetime, ordering, duplicate IDs and recovery.
It starts a native process per request. The bundled Electron runtime is
prepared only when evaluation is requested; literal-only listing requires
neither runtime extraction nor a Node subprocess. Evaluation can use a second
native invocation, reusing the first invocation's parsed records.

| Resource | Bound and release rule |
| --- | --- |
| Saved records | At most 32 MiB of versioned postcard data per project |
| Complete ordered answer | At most 32 MiB per project; oversized answers are not saved |
| Project saves | At most 32 root-digest directories under `canonic-workbench-index/` in the system temporary folder; evict inactive directories after seven days and oldest directories beyond the count bound during index requests |
| Native invocation | Default deadline of 120 seconds per invocation; terminate the process tree before falling back |
| Native stdout | At most 128 MiB per invocation; exceeding it terminates the process tree |
| Captured stderr | At most 4,000 characters for failure diagnostics |

Saved indexes belong outside the project. They are not authoring files or
portable build outputs. A missing, failing, malformed or timed-out native
index emits `previews.index.fallback` and uses the worker's index. Successful
native listing does not start the compile worker; later rendering and
pre-warming retain their own worker lifecycle.

### Distribution

Every supported VSIX target contains its matching native executable. Packaging
builds locked release dependencies for that Rust target or uses an explicitly
provided prebuilt executable. Missing target/linker prerequisites stop
packaging. Ship the executable, excluding Rust source, Cargo caches and test
reports. [Code quality](../code-quality.md) owns the build and verification
policy; [the native README](../../server/README.md#checks-and-packaging) owns
developer commands.

## Acceptance and evidence

These criteria describe observable behavior. Existing automated evidence is
named below; a test's presence does not establish performance or platform
acceptance.

| ID | Acceptance criterion | Existing evidence |
| --- | --- | --- |
| PI-01 | Native and compiler answers agree for literal/computed previews, docs, invalid definitions, duplicate IDs, custom globs and discovery order | [Adapter parity tests](../../server/node/src/modules/page-index/index.test.ts) and [answer tests](../../server/node/src/modules/page-index/answer.test.ts) |
| PI-02 | Literal listing requires no JavaScript executable; executable definitions use evaluation | Adapter tests; [literal unit and property tests](../../server/src/literal.rs) |
| PI-03 | Refresh observes additions, removals, renames, retitles and changed imported list fields | [Native CLI integration](../../server/tests/integration/main.rs) and adapter dependency-invalidation test |
| PI-04 | Invalid saves rebuild, only matching generations reuse the complete answer, failed saves preserve successful listing, and a corrupt ordered answer does not lose pages | [Saved-index unit test](../../server/src/saved.rs), native CLI generation/handoff/save-failure tests and adapter tests |
| PI-05 | Native failure or timeout logs a diagnostic and returns the fallback answer | Adapter missing/failing-binary and timeout test |
| PI-06 | Trust and disabled-preview controls prevent project evaluation | [Preview server integration tests](../../server/node/preview.test.js) |
| PI-07 | Cache eviction, byte bounds and process cleanup follow the resource table | Implemented bounds; dedicated boundary/eviction and descendant-cleanup assertions remain verification gaps |
| PI-08 | Installed platform binary lists pages with the same public manifest shape | Host packaging/install probe recorded in [performance](performance.md#page-listing); acceptance on all six targets is not established by that probe |

## Remaining work

The implemented design uses directory-stamp reuse, literal parsing and the
compiler evaluator. [Performance](performance.md#page-listing) owns measured
results and target gaps; [lab evidence](../../labs/instant-first-page/README.md#stage-1-report)
supports the architecture without defining the shipping interface.

- Definition watching remains the gap owned by
  [sidebar placement](../previews.md#sidebar-placement). The index observes
  changes when requested; it does not initiate a refresh.
- The worker's index and pre-warming remain independent. Sharing the native
  file list with them belongs to the compiler-host work.
- A resident index and watcher may reduce process and transport costs; they
  are open design choices, not implemented behavior.
- Studio has no native-library integration contract. The presence of a Rust
  library does not establish an embeddable indexing API.
- Metadata stamps do not establish content identity when both modification
  time and size are preserved. Stronger invalidation would need a separate
  design and measurement.
