# Workbench server

`workbench-server` is the Workbench server's Rust binary, one stable Rust
package with a library and two commands:

- `serve`, the front door: it owns the space's public loopback port, forwards
  every request to the Node HTTP server under `node/`, keeps the page index
  resident and reports page-tree changes;
- `index`, one page-index transaction per process, which the Node server
  uses when the front door is not running.

The HTTP server and compiler worker are under `node/`. Their move into this
binary is planned in phases in the [server spec](../specs/server.md).

## Architecture

| Owner | Responsibility |
| --- | --- |
| `main.rs`, `lib.rs`, `cli.rs` | Exit status, deliberate library entry point, arguments and bounded standard-stream transport |
| `index.rs`, `index/` | One index transaction; private stages own definition acquisition, cache decisions and phase diagnostics |
| `definition.rs`, `definition/` | Pure literal-definition rules: compatibility with JavaScript, ordered validation and list projection |
| `literal.rs` | Side-effect-free SWC extraction of literal values |
| `discovery.rs` | Filesystem traversal, directory reuse and native matching policy |
| `evaluate.rs` | Typed select/evaluate requests and ownership of the evaluator child |
| `saved.rs` | Bounded versioned persistence and atomic publication |
| `protocol.rs` | Response representation and conversion of records to the CLI answer |
| `serve.rs`, `serve/` | The front door: options and the public listener; `proxy.rs` forwards requests and bridges upgrades, `messages.rs` carries line-delimited JSON on the standard streams, `resident.rs` holds the index in memory with its watcher |
| `model.rs`, `files.rs`, `paths.rs` | Shared input/record types, filesystem observations and pure lexical path helpers |

Pure definition and path rules do not read files or start processes. The CLI
composes the evaluator with the index workflow. Adapters accept their own
inputs rather than importing application internals. Shared observations stay
outside capabilities; private stage helpers stay with their owner. The library's
public API is the CLI-oriented `run`; it does not expose an embedding API.

The Node server's adapters are `node/src/modules/front-door/index.ts`, which
starts `serve`, and `node/src/modules/page-index/index.ts`.

The server's public adapter is `node/src/modules/page-index/index.ts`. It owns the
binary path, temporary storage, runtime preparation, deadline, process-tree
cleanup, discovery ordering, duplicate IDs and fallback. No project code runs
before the server's workspace trust check.

## Front door

`workbench-server serve --upstream <port> --ports <port,...>` listens on
127.0.0.1 at the first free port of the list (0 takes any) and forwards every
request to the Node server on `--upstream`: methods, headers, streamed bodies
both ways, `HEAD`, and upgraded connections. An unreachable upstream answers
502. Its standard streams carry one JSON message per line:

| Direction | Message |
| --- | --- |
| Out, once | `{"type":"listening","port":N}` |
| In | `{"type":"index","id":N,"request":{...}}`, the request below |
| Out | `{"type":"answer","id":N,"ok":true,"response":{...}}`, or `ok: false` with `error` |
| Out | `{"type":"changed"}` when the page tree changed on disk |
| Out | `{"type":"error","error":"..."}` for an unreadable message |

It exits when its standard input closes. The Node adapter starts it in its
own process group and kills the group when it exits, so an evaluator it left
running goes with it. An answer over 128 MiB is sent as `ok: false`, the
bound the one-process index has on its output. The first index request starts a
recursive watcher on its root. While the watcher is healthy and nothing in
the answer's scope has changed, a request for that answer is reused without
reading the disk. The scope is the folders the walk starts from, the files the
saved definitions read, the settings files and `workbench.config.ts`; without
a saved index for this answer (its save failed), or with a definition whose
reads are unknown, every change outside `.git` counts. Changes inside folders
the walk skips count only for files a definition read, and a watcher rescan
(dropped events) always counts. A change in scope re-indexes in the background after
150 ms of quiet and sends `changed` when the listed records differ from the
last ones announced, so a save that edits a component reports nothing, and a
change another request fetched first is still announced. A watcher that
cannot start leaves every request on the full index check and reports no
changes.

## Index protocol

Spawn `workbench-server index`, or send the request through the front door,
with one JSON object:

```json
{
  "root": "/absolute/acme",
  "saved": "/temporary/index.postcard",
  "identity": "binary-and-options-digest",
  "evaluator": "/workbench/server/node/preview/evaluate-definitions.cjs",
  "node": "/runtime/node",
  "options": { "include": ["**/*.workbench.ts", "**/*.workbench.tsx"] },
  "walk": false,
  "deadline_ms": 120000
}
```

`deadline_ms` bounds each evaluator run (120 seconds by default): at the
deadline the evaluator is killed and the request fails. `walk` is a
diagnostic full-walk override. The default checks saved directory
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

The pinned Rust 1.99.0 toolchain includes rustfmt, Clippy and LLVM tools.
Install the same test, dependency and coverage tools used by the other Rust
packages:

```sh
cargo install --locked cargo-nextest@0.9.146 cargo-deny@0.20.2 cargo-llvm-cov@0.9.1 cargo-insta@1.49.0
```

From this directory:

```sh
make check
make coverage
```

`make check` runs all gates below with locked dependencies and all features.
`make coverage` writes the HTML report to `target/llvm-cov/`.
From the repository root, use `pnpm --dir packages/workbench run check:server`
or `pnpm --dir packages/workbench run coverage:server`.

```sh
cargo fmt --check
cargo clippy --locked --all-targets --all-features -- -D warnings
cargo nextest run --locked --all-features
cargo test --doc --locked --all-features
cargo doc --locked --all-features --no-deps --document-private-items
cargo deny check
```

Formatting uses rustfmt defaults. `Cargo.toml` owns the shared Rust, Clippy
and rustdoc lint policy; `clippy.toml` permits test-only unwraps.
`.config/nextest.toml` reports every failure and terminates tests after two
minutes. `deny.toml` uses the shared license allowlist with the per-crate
exceptions listed in [code quality](../specs/code-quality.md#dependency-and-artifact-policy). The extension release workflow runs
the native gates before packaging.

Unit tests use proptest to exercise escaped strings, nested literals and the
boundary that sends executable definitions to Node. The page-list entry has an
insta JSON snapshot alongside behavioral assertions. Review intended output
changes with `cargo insta review`. The integration tests use assert_cmd,
assert_fs and predicates with a temporary project and saved index that are
cleaned up together. The front door's tests run the built binary against
scripted upstreams for forwarding, streaming, `HEAD`, 502, upgrades and exit,
and against a temporary project for the resident index and change reports. Coverage reports have no percentage gate, matching TUI
and Shield.

`make build` builds the local release executable. `CARGO_FLAGS=--offline`
uses cached dependencies for Cargo build, lint, test, documentation and coverage
commands; cargo-deny manages its own advisory cache.

`node ../scripts/build-server.cjs` builds the host release executable into
`bin/<VSIX target>/`. `--debug` builds the test executable. Packaging calls
the same builder with a Rust target triple and ships only that target's binary.
To package with a prebuilt executable, set `WORKBENCH_SERVER_BINARY` to its
path. Cross builds require the target's Rust standard library and linker;
failure stops packaging with those prerequisites in the error.

macOS can spend about 0.8 seconds checking a newly placed executable on its
first launch, as observed in the instant first page lab. Subsequent launches
avoid that check, like the bundled runtime's existing first-launch delay.
