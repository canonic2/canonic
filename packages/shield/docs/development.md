# Developing Shield

Shield is one Cargo package (`canonic-shield`), library `shield`, and executable
`shield`. Rust 1.99.0 is pinned with rustfmt, Clippy and llvm-tools. Cargo.lock
owns dependency versions; there is no root Cargo workspace.

## Fast iteration

From this package directory:

```sh
make dev
make test-focus FILTER='policy::'
# Select integration tests explicitly when needed:
make test-focus FILTER='selected_test_name' TEST_TARGET='--test integration'
```

`dev` writes an incremental build to `target/debug/shield`; run it directly to
try changes. `test-focus` uses nextest with a required substring filter and
defaults to library tests. Replace the integration example's placeholder with
a real test name; nextest fails if nothing matches.

Keep `target/` between iterations to reuse Cargo's cache. Use release builds
for performance measurements.

## Source ownership

Shield separates policy decisions from filesystem observations, host adapters,
and operator workflows. Start with the module for the behavior you are changing:

| Area | Source |
| --- | --- |
| Operator commands and routing | `src/cli/public.rs`, `src/cli.rs`, `src/lib.rs` |
| Policy documents, selectors, requirements | `src/policy.rs`, `policy/model.rs`, `policy/rules.rs`, `policy/argument_rules.rs` |
| Typed requests and resource-evidence interface | `policy/normalized.rs`, `policy/resources.rs` |
| Filesystem observations and supported command effects | `src/location.rs`, `location/text.rs`, `location/node.rs` |
| Shell parsing, expansion, control flow, and PID provenance | `src/shell.rs` and `shell/` |
| Declared directory options | `policy/directories.rs` |
| Compiled plans, indexes, and policy comparison | `policy/compiled.rs`, `policy/index.rs`, `policy/partition.rs`, `policy/profile_comparison.rs`, `policy/symbolic.rs` |
| Project composition, captured source, and evaluation | `src/configuration.rs` and `configuration/` |
| Shared project, policy and request inspection | `review/project.rs`, `review/history.rs`, `review/approvals.rs` |
| Pack discovery, acquisition and candidate preparation | `review/packs.rs`, `review/packs/`, `review/registries.rs` |
| Confirmed publication and project-file repair | `review/publication.rs`, `review/publication/projections.rs`, `review/changes/diff.rs` |
| CLI questions, reviews and confirmation | `review/operator/` |
| Agent configuration ownership and repair mechanics | `review/integrations.rs`, `review/integrations/`, `review/path_access.rs` |
| Approval clocks and request-specific grants | `review/clock.rs`, `review/read_approval.rs`, `review/run_approval.rs` |
| Durable source, disposable cache, and history | `review/storage/sources.rs`, `review/cache.rs`, `review/storage/history.rs`, `history/correlation.rs` |
| Agent payloads, setup codecs, and replies | `src/agents.rs`, `agents/claude_code.rs`, `agents/codex.rs`, `agents/setup.rs` |
| Shared hook dispatch and feedback | `agents/hook.rs`, `agents/feedback.rs`, `src/agent_message.rs` |
| Host-specific MCP confirmation | `agents/review.rs`, `agents/*/review.rs` |
| MCP startup, transport handler, and output | `src/mcp_io.rs`, `src/mcp.rs`, `mcp/response.rs`, `mcp/messages.rs` |
| MCP tools, schemas, snapshots, and cursors | `review/mcp/` |
| Bundled Markdown guides | `build.rs`, `review/mcp/documentation.rs` |
| Runtime snapshots and protected handoff | `src/runtime.rs`, `review/runtime/consumer.rs`, `src/enforcement.rs` |
| Supported native launches and cleanup | `src/execution.rs` and `execution/` |
| Document validation and pack fixtures | `src/application.rs` |
| Private process diagnostics | `src/diagnostics.rs` |

CLI and MCP map their inputs into shared review operations. History queries use
an application-owned query type; pack operations expose direct list, show, search,
info and preparation functions. Shared owners can read files and acquire registry
artifacts, including retaining verification checkpoints, without opening a terminal.
CLI prompts and host confirmation run before calling the common publication
workflow. MCP retains its own paging, diagnostic handles and response projection.

The policy core performs no filesystem I/O and contains no command permission
tables. Packs and custom rules supply permissions; `location/` supplies fresh
request facts. Shell analysis uses submitted source and available context,
without attesting execution.

`build.rs` compresses the guides into one DEFLATE bundle in `OUT_DIR`, with a
page-range index. Docs tools decompress it in memory when called. Both steps
use pure Rust code; no host compression tools or shared libraries are needed.

The `canonic-registry` dependency handles registry identity, authentication,
acquisition, checkpoints, resolution, and UTC timestamps. Shield owns pack
declarations and the review that activates acquired artifacts. `canonic-mcp`
handles framing, lifecycle, validation, cancellation, deadlines, and confirmation
requests; Shield's handler supplies its tools and limits.

Agent adapters translate vendor payloads and setup formats. They cannot bypass
core policy or activation checks. To add an agent, implement its adapter and
registry entry, then verify payload mapping, setup, and real host behavior.
CLI detection belongs in the adapter's `is_installed` method; it inspects PATH
without starting the tool.

`adapters/` contains private Node 24+ ESM modules: `process-transport/` handles
subprocess I/O and cancellation, `enforcement-response/` validates receipts,
and `claude-read-only/` maps SDK callbacks. `adapters/sdk-tests/` pins the SDK
dependencies. Rust integration tests share `tests/integration/main.rs`.

## Check tools

Install the developer tools:

```sh
cargo install --locked cargo-nextest@0.9.146 cargo-deny@0.20.2 cargo-llvm-cov@0.9.1 cargo-insta@1.49.0
```

Tests use assert_cmd, assert_fs, predicates, insta, and proptest. Tool versions
and dependencies are pinned in the package configuration.

## Checks

The default nextest profile runs four tests at a time. Run process-heavy Node,
SDK and Rust suites separately so fixture startup does not compete across runners.
Do not rebuild a shared executable while installation fixtures run: registrations
pin its digest, and replacing it changes the fixture's protection diagnostics.
In an agent sandbox, request host access before running integration checks:
PTY fixtures need controlling terminals and process signals, HTTP/TLS fixtures
need loopback listeners, and cargo-deny needs its advisory cache. A sandbox
permission refusal requires the same check with host access; product policy
changes cannot grant that access.

Run the [Make targets below](#individual-rust-verification) from this package
directory, where rustup selects the pinned toolchain.

`cargo test --locked` also runs the behavioral tests. `cargo llvm-cov nextest
--locked` produces a coverage report; it is not a coverage threshold. Cargo
nextest, deny, and llvm-cov are optional developer tools installed separately.
Use `~/.cargo/bin/cargo` when Cargo is not on PATH and `--offline` only with a
populated dependency cache.

| Area | Configuration and behavior |
| --- | --- |
| Lint | Manifest-owned unsafe-code prohibition, unreachable-public-item warnings, Clippy pedantic/unwrap/self-named-module checks; warnings fail the check |
| Test runner | nextest reports every failure; slow tests are flagged after 30 seconds and terminated after two minutes |
| CLI fixtures | assert_cmd/predicates check status/output; assert_fs owns temporary projects and cleanup |
| Property tests | proptest exercises unknown-command refusal and literal argv exact-match boundaries |
| Snapshots | insta records command help and settled human output under tests/integration/ |
| API docs | `[lints.rustdoc]` in Cargo.toml: a doc comment linking to a missing or private item fails `cargo doc` |
| Dependencies | cargo-deny checks advisories, licenses, duplicate versions, and crates.io-only sources; wildcard versions fail |
| Coverage | cargo-llvm-cov reports line, region, and function coverage; no minimum threshold |

Clippy's `too_many_lines` remains enabled. Use narrow
`#[expect(..., reason = "...")]` annotations when justified. Public fallible APIs
must document their errors.

Failure fixtures control transport timing while keeping evaluation and subprocess
I/O real. Wait for readiness before measuring deadlines, and keep setup budgets
separate from cleanup assertions. SDK acceptance uses production deadlines.

## Output review and deeper testing

For intended output changes, run `cargo insta test`, then `cargo insta review`
and commit the reviewed snapshots. Snapshots complement status and filesystem
assertions; they do not replace them. Keep paths and secrets out of snapshots;
insta filters are available when context-dependent output is implemented.
Human-output fixtures drive the shared TUI in an owned terminal and snapshot
settled reviews, choices and results. Separate terminal tests verify live regions,
input safety and mode restoration.

`cargo llvm-cov nextest --locked --html` writes a browsable report under
target/llvm-cov/html/. Generated API documentation is written to target/doc/. Reports and generated
docs remain under ignored target/.

## Local build and adapter checks

From the repository root:

```sh
make -C packages/shield build
npm --prefix packages/shield/adapters test
npm --prefix packages/shield/adapters/sdk-tests test
```

`make -C packages/shield install-local` installs into `~/.local/bin`; `PREFIX`
selects another root. From this package use `make build` and `make install-local`.
`CARGO` overrides Cargo discovery and `CARGO_FLAGS=--offline` uses cached dependencies.
Make prefers `~/.cargo/bin/cargo` so rustup selects the pinned toolchain,
falling back to Cargo on PATH.

## Debug builds and diagnostics

`make -C packages/shield build-debug` builds `target/diagnostic/shield` with release
optimization, debug symbols, debug assertions and automatic JSONL logging. Native
hook deadlines also apply to diagnostic builds. `make -C packages/shield install-local-debug`
installs that build into `~/.local/bin`. `make -C packages/shield install-local`
installs the release build, which has logging disabled by default. After replacing
the installed executable, run `shield mcp setup --agent codex --project /path/to/project`
(or `--agent claude`) to refresh its pinned digest, then restart the agent.

Automatic logs live in the OS temporary directory under
`shield-logs/shield-<timestamp>-<pid>.jsonl`. On macOS, inspect them with
`ls -lt "${TMPDIR%/}/shield-logs"`. Each native hook gets its own file; the MCP
server keeps a file for its process lifetime. Operator commands print their log
path on stderr. Native hooks and MCP omit that startup message so protocol output
and ordinary agent feedback remain clean.

MCP frame logging uses a separate worker with an eight-frame queue. The
dispatcher submits frames without waiting for parsing or file writes; a full
queue drops diagnostic frames. Tool names are cached once per process. The
logging worker drains accepted frames and is joined when its observer is dropped,
so slow storage can delay shutdown but does not block live ping or cancellation
dispatch. Frame sizes remain bounded by the MCP input and output limits.

Both builds accept `--debug-log-dir DIRECTORY` to enable logging into another
directory and `--no-debug-log` to disable it. `SHIELD_DEBUG_LOG_DIR` also selects
the directory for child processes that inherit it; export it before starting the
agent. Hosts can filter child environments. The CLI directory overrides the environment, and opt-out overrides the
environment and debug default. Shield uses a directory rather than a single
file because concurrent native callbacks start separate processes.

Files are uniquely created, use mode 0600 on Unix, and stop at 8 MiB with a limit
marker. Logs remain until you delete them or the OS clears temporary files; total
directory size is not automatically capped. A logging failure reports a warning
on stderr and disables logging without changing authorization or transport output.

Records include elapsed milliseconds and PID/version/build type,
native agent/event/tool and session/turn/tool-call IDs, captured input bytes,
input validity and overflow, project/snapshot preparation, evaluation, native
handoff and history stage timings, policy decision/fallback/disposition, completion
history status, response event/decision/context shape, and MCP method/tool/ID,
request/response sizes and error status. Commands, arguments, tool results,
policy content, credentials and raw error text are omitted. Large truncated hook
input cannot supply parsed IDs; its overflow and captured size are still recorded.
Pair input/output records by PID and MCP messages by request ID to inspect timing.
These logs are diagnostic evidence, separate from request history and execution
attestation. `RUST_BACKTRACE=1` enables panic backtraces on stderr; debug symbols
also support native debuggers.

## Adapter and host checks

The dependency-free adapter suite requires the actual package release executable.
The SDK runner executes one test file at a time; each file retains its own
concurrency assertions and owns its host processes and loopback provider.
Codex fixtures disable plugin discovery in their isolated host so background
marketplace clones cannot race project cleanup. Their teardown closes the local
provider before removing the project, including after a failed assertion.
The SDK suite uses locked MCP SDK 1.32.1 and Claude Agent SDK 0.3.292 dependencies.
Its temporary projects and scripted loopback provider exercise canonical MCP
inspection/proposals, dense deadline/cancellation behavior, supported native reads,
exact retries, state/operator refusals, host discovery and completion privacy. Native command-hook fixtures also exercise
Claude native Ask permission callbacks (approval and refusal), policy Deny bypassing
ordinary approval, Codex approval-unavailable blocking, explicit denial, startup
failure, and the host's timeout continuation boundary. Confirmed automatic Ask
allowance has distinct history/dispatch assertions. Codex's unsupported Ask and
ignored approval-field rewrites have separate disposable marker probes.
PermissionRequest boundary tests cover complete-context Allow, Ask abstention,
Deny, missing IDs and workdir, mixed batches and event-pinned input failures.
Codex shell approval with an omitted workdir uses reported session cwd and follows
policy Allow, Ask or Deny. Missing context remains recorded independently of handoff.
Do not describe callback translation as a verified human approval flow.
`adapters/sdk-tests/claude-interactive.test.mjs` separately runs the installed
Claude CLI in an owned PTY with the ordinary terminal renderer. Its local provider
requests policy apply in `auto` and `bypassPermissions`; actual keyboard input
answers Accept, Decline and Escape. The fixture observes the host's permission
mode and verifies unchanged policy while input is withheld. It uses neither an
SDK elicitation callback nor an `Elicitation` hook. Run it independently with
`node --test --test-concurrency=1 packages/shield/adapters/sdk-tests/claude-interactive.test.mjs`
after `make -C packages/shield build`. It requires the installed `claude` command,
Python 3, PTYs, loopback access and permission to reap its owned child processes.
For a fresh checkout install SDK test dependencies with
`npm --prefix packages/shield/adapters/sdk-tests ci --ignore-scripts --no-fund --no-audit`.
Native host checks are verified on macOS; Linux/Windows behavior is unverified.

## Individual Rust verification

Choose the checks needed for your change; there is no aggregate target.
`docs` generates HTML in `target/doc/`; `doctest` runs documentation examples.
Rustdoc may recheck dependencies even with `--no-deps`, which only suppresses
their generated pages. Keep Cargo caches between runs.

| Category | Command from this package |
| --- | --- |
| Apply formatting | `make format` |
| Verify formatting | `make format-check` |
| Strict Clippy | `make lint` |
| Type checking | `make types` |
| Tests | `make test` |
| Documentation examples | `make doctest` |
| API reference HTML | `make docs` |
| Dependency audit | `make deps` |

`make format FORMAT_FILES='src/cli/public.rs'` formats the named files and their
out-of-line child modules. Name only leaf files (files with no out-of-line child
modules) to keep edits limited to those files. For example, naming `src/agents.rs`
also formats modules under `src/agents/`.
