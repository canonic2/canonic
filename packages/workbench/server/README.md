# Workbench server

`workbench-server` is the Workbench server's Rust binary, one Rust package
with a library and these commands:

- `serve`, one space's whole HTTP server: it owns the space's public loopback
  port, serves Workbench's files and the project's, compiles and serves
  previews with Rspack's Rust crates, assembles the page tree, answers the
  space's routes, keeps the page index resident and reports page-tree
  changes. The process that starts it, its host, answers what belongs to the
  editor or the capture helper ([server spec](../specs/server.md));
- `init`, `list`, `check` and `build <project> [output]`, the command-line
  builds without a server: authoring types and a manifest, what the page
  index finds, every preview built with failures reported, and the portable
  browser build written to a folder. Project code runs on `--node` or the
  `node` on the `PATH`; `--product` names Workbench's folder when the binary
  is not inside it;
- `index`, one page-index transaction per process;
- `config`, a folder's resolved manifests as JSON.

The host is `node/src/modules/host/`, used by the extension and by
`node/server.js`'s standalone command.

## Architecture

| Owner | Responsibility |
| --- | --- |
| `main.rs`, `lib.rs`, `cli.rs` | Exit status, deliberate library entry point, arguments and bounded standard-stream transport |
| `index.rs`, `index/` | One index transaction; private stages own definition acquisition, cache decisions and phase diagnostics |
| `definition.rs`, `definition/` | Pure literal-definition rules: compatibility with JavaScript, ordered validation and list projection |
| `literal.rs` | Side-effect-free SWC extraction of literal values |
| `discovery.rs` | Filesystem traversal, directory reuse, the walk's skip and `.gitignore` rules, and native matching policy |
| `evaluate.rs` | Typed select/evaluate requests and ownership of the evaluator child |
| `saved.rs` | Bounded versioned persistence and atomic publication |
| `protocol.rs` | Response representation and conversion of records to the CLI answer |
| `serve.rs`, `serve/` | The server: options and the public listener; `router.rs` sends each request to previews, files or routes, `files.rs` serves files as they are, `previews.rs` answers the preview routes, `routes/` the space's routes, `messages.rs` carries line-delimited JSON on the standard streams, `resident.rs` holds the index in memory with its watcher |
| `space.rs`, `space/` | One space's services composed, with what the page tree adds from Storybook and Simulators; `docs.rs` docs pages on the native compiler, `export.rs` the export's capture, docs and portable sides |
| `catalog.rs`, `catalog/` | The page tree: the index request, its answer read in discovery order, previews and docs pages placed in the tree |
| `host.rs`, `diagnostics.rs` | Questions and notices to the host; bounded diagnostic records with the session budget |
| `config.rs`, `config/` | Manifest reading, checking and resolution; `edit.rs` the page form's and size dialogs' edits |
| `docs.rs`, `docs/` | Docs pages: Markdown reading and placements, rendering, highlighting, the page document, the service, export sources and portable pages |
| `storybook.rs`, `simulators.rs` | Storybook indexes, detection and the Storybook the server runs; booted Simulators, their catalog and Sandbox input |
| `proxies.rs` | The implementation proxies |
| `capture.rs`, `handoff.rs`, `agent_view.rs` | Capture requests checked and screenshots saved; handoff prompts; the canvas view agents read |
| `export.rs`, `export/`, `portable.rs` | The design export's plan, capture, sources, archives, PDF and jobs; the portable browser build |
| `build.rs` | The command-line builds |
| `compiler.rs`, `compiler/` | The preview compiler: `previews.rs` turns definitions and docs lenses into entries and owns generations and watching; `host.rs` runs one profile's Rspack compiler on its builder task; `options.rs` holds Rspack's options; `entries.rs`, `client.rs` and `html.rs` generate entry, browser and HTML modules; `loaders.rs`, `vue.rs` and `node_loader.rs` are Workbench's loaders; `plugins.rs` and `plugins/` the project's rules and plugin capabilities; `docs.rs` and `exports.rs` docs lenses; `astro.rs` Astro renders; `portable.rs` production builds; `overlay.rs` generated modules over the file system |
| `node.rs` | The Node evaluator: one long-lived child per space for project JavaScript, matched by request ID on its standard streams |
| `order.rs` | Names ordered as the JavaScript `localeCompare` the Node side uses |
| `model.rs`, `files.rs`, `paths.rs` | Shared input/record types, filesystem observations and pure lexical path helpers |

Pure definition and path rules do not read files or start processes. The CLI
composes the evaluator with the index workflow; `serve` composes the
space's services with their routes. Adapters accept their own
inputs rather than importing application internals. Shared observations stay
outside capabilities; private stage helpers stay with their owner. The library's
public API is the CLI-oriented `run`; it does not expose an embedding API.

The Node evaluator's script is `node/preview/evaluator.cjs`; the page index's
is `node/preview/evaluate-definitions.cjs`. No project code runs before the
server's workspace trust check.

## Serve

`workbench-server serve --root <project> --product <workbench> [--ports
<port,...>] …` listens on 127.0.0.1 at the first free port of the list (0
takes any). Its options, standard-stream messages and routes are in the
[server spec](../specs/server.md#the-server-and-its-host).

It exits when its standard input closes. The host starts it in its own
process group and kills the group when it exits, so an evaluator it left
running goes with it. The compiler's routes, outputs and persistent cache
are described in the [preview pipeline](../specs/preview-pipeline/README.md). An answer over 128 MiB is sent as `ok: false`, the
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

Spawn `workbench-server index`, or send it to `serve` as an `index` message,
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
runtime; the server asks its host for one and repeats the request. Literal-only
projects need neither runtime extraction nor a Node subprocess.

Stdout is JSON with `records`, `timings`, `needs_evaluator`, `generation`
and `reuse_answer`. Records are
keyed by relative `file`, with `docs` and `entry` (`Ok`: projected entry JSON,
or `Err`: compiler error text). The catalog orders path segments with the
compiler's `localeCompare`, then checks IDs across previews and docs into
`{ previews, docs, errors }`, and keeps the last answer with its generation.
An unchanged request with the same `cached_generation` returns `reuse_answer`
and timings, so that answer needs no projection or ordering. Timings report load, walk,
check, parse, evaluate, save and total milliseconds, directory reads and
definition counts. `previews.indexed` adds server elapsed time.

Plain include globs use globset with literal path separators. Other patterns
are selected by the compiler's picomatch in the evaluator, with selection
reused while the directory tree is unchanged. The evaluator uses the
compiler's shared validation and projection. Failed evaluations retry rather
than caching an incomplete dependency graph.

Saved data is versioned postcard, capped at 32 MiB per project, atomically
renamed and scoped to the project root digest in the system temporary folder.
The server retains at most 32 projects and evicts inactive projects after
seven days on the next index request. A changed binary or compiler inputs
invalidate the save. Read or write failures never prevent a page list.

## Checks and packaging

`rust-toolchain.toml` pins Rspack's own toolchain at the pinned crates,
`nightly-2026-04-16`, with rustfmt, Clippy, LLVM tools and the standard
library's source; Rspack's crates do not build on stable. The first build
compiles Rspack's crates and takes several minutes. Dev builds, which the
tests run, optimize dependencies, since Rspack and SWC compile previews far
more slowly unoptimized.
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
cleaned up together. The server's tests run the built binary against a
temporary project for its files, configuration, a question to its host,
exit on closed input, the resident index and change reports. Coverage reports have no percentage gate, matching TUI
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
