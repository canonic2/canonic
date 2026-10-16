# Workbench native index

`workbench-native` lists definitions for the server without starting the
compile worker. It is one stable Rust package with a library and an `index`
command. The compiler host can join this package before a workspace is needed.

The server's public adapter is `src/modules/page-index/index.ts`. It owns the
binary path, temporary storage, runtime preparation, deadline, process-tree
cleanup, discovery ordering, duplicate IDs and fallback. No project code runs
before the server's workspace trust check.

## Protocol

Spawn `workbench-native index` and send one JSON object on stdin:

```json
{
  "root": "/absolute/acme",
  "saved": "/temporary/index.postcard",
  "identity": "binary-and-options-digest",
  "evaluator": "/extension/preview/evaluate-definitions.cjs",
  "node": "/runtime/node",
  "options": { "include": ["**/*.workbench.ts", "**/*.workbench.tsx"] },
  "walk": false
}
```

`walk` is a diagnostic full-walk override. The default checks saved directory
stamps. With `node` empty, a response with `needs_evaluator: true` requests a
runtime; the adapter prepares Electron and repeats the request. Literal-only
projects need neither runtime extraction nor a Node subprocess.

Stdout is JSON with `records`, `timings`, `needs_evaluator`, `generation`
and `reuse_answer`. Records are
keyed by relative `file`, with `docs` and `entry` (`Ok`: projected entry JSON,
or `Err`: compiler error text). The adapter orders path segments with the
compiler's `localeCompare`, then checks IDs across previews and docs and
returns exactly `{ previews, docs, errors }`. It atomically saves the complete
ordered answer beside the postcard file, tagged with the native generation.
An unchanged request with the same `cached_generation` returns `reuse_answer`
and timings, so the saved answer needs no projection or ordering. A missing
or corrupt answer requests full native records. Timings report load, walk,
check, parse, evaluate, save and total milliseconds, directory reads and
definition counts. `previews.indexed` adds server elapsed time.

Plain include globs use globset with literal path separators. Other patterns
are selected by the compiler's picomatch in the evaluator, with selection
reused while the directory tree is unchanged. The evaluator uses the
compiler's shared validation and projection. Failed evaluations retry rather
than caching an incomplete dependency graph.

Saved data is versioned postcard, capped at 32 MiB plus a 32 MiB ordered
answer per project, atomically
renamed and scoped to the project root digest in the system temporary folder.
The adapter retains at most 32 projects and evicts inactive projects after
seven days on the next index request. A changed binary or compiler inputs
invalidate the save. Read or write failures never prevent a page list.

## Checks and packaging

From this directory with Rust 1.99.0:

```sh
cargo fmt --check
cargo clippy --all-targets -- -D warnings
cargo test
cargo doc --no-deps --document-private-items
cargo deny check
```

`node ../scripts/build-native.cjs` builds the host release executable into
`bin/<VSIX target>/`. `--debug` builds the test executable. Packaging calls
the same builder with a Rust target triple and ships only that target's binary.
To package with a prebuilt executable, set `WORKBENCH_NATIVE_BINARY` to its
path. Cross builds require the target's Rust standard library and linker;
failure stops packaging with those prerequisites in the error.

macOS can spend about 0.8 seconds checking a newly placed executable on its
first launch, as observed in the instant first page lab. Subsequent launches
avoid that check, like the bundled runtime's existing first-launch delay.
