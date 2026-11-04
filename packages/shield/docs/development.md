# Developing Shield

Shield is one Cargo package (`canonic-shield`), library `shield`, and executable
`shield`. Rust nightly-2026-04-16 (rustc 1.97) is pinned with rustfmt, Clippy and llvm-tools. Cargo.lock
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

## Native Linux development

GNU/Linux builds support x64 and ARM64 with the pinned Rust toolchain, a GNU C
compiler and ordinary Unix file permissions, terminal leases and native hooks.
Shield includes MCP Kit, Registry Kit and TUI Kit; Process Kit is not used.
The CLI needs no Node installation. Interactive commands require terminal
stdin/stdout and a cursor-capable `TERM`; hooks and MCP stdio remain noninteractive.

From this package directory, `make dev` builds `target/debug/shield`.
The distribution helper additionally requires Python 3.11+ and GNU binutils:

```sh
python3 scripts/release-linux.py build --architecture x64
python3 scripts/release-linux.py package --architecture x64
```

Outputs are `target/x86_64-unknown-linux-gnu/release/shield` and
`dist/shield-<version>-x86_64-unknown-linux-gnu.tar.gz`, with a `.sha256` sidecar.
`CARGO_TARGET_DIR` selects another build directory; `--output-directory` selects
another package directory. Existing archives/checksums are refused. The helper
remaps publisher paths in Rust/C code, checks the ELF architecture and scans the
binary for publisher paths before accepting a build. Clear custom Rust/C flags
before using it. Package after rebuilding changed code or dependencies.

For a cross-build from x64, install an `aarch64-linux-gnu-gcc` cross-compiler
and its GNU target libraries, then add the pinned Rust target:

```sh
rustup target add --toolchain nightly-2026-04-16 aarch64-unknown-linux-gnu
python3 scripts/release-linux.py build --architecture arm64
python3 scripts/release-linux.py package --architecture arm64
```

An ARM64 Linux host can build that target with its native C compiler. Cross-builds
do not establish native ARM64 acceptance. The packager includes Canonic's license,
resolved normal/build dependency notices and Rust standard-library notices.
It normalizes archive ownership and indexes notices in `BUILD.json`, alongside
the executable checksum, versioned glibc symbol requirements and a conservative
minimum glibc baseline at least as new as the selected compiler's target libc.
The helper locates `libc.so.6` through that compiler, checks its architecture
and reads its release version without executing it. An unavailable or
unrecognized target libc refuses the build, including cross-builds.
These are workspace candidates without source-revision attestation.

### Linux installation and compatibility

Extract the archive, then copy its `shield` executable to a stable folder on PATH,
for example `~/.local/bin/shield`, with mode 755. Check `shield --version` and
follow [Your first Shield project](getting-started.md). Close Shield and connected
agents before replacing the executable; repair each project's registrations with
`shield mcp setup --repair` after an update or move. To remove an installation,
run `shield mcp remove` for each connected agent in each project, then delete the
executable. Preserve project `.shield` folders to retain policy and history.

The GNU executable's minimum glibc version depends on the target toolchain and is
recorded in `BUILD.json`. Build on the oldest intended supported GNU system to
target older hosts; a successful build on a new Ubuntu release does not prove
compatibility with older distributions. The bounded native runner additionally
requires a supported glibc 1.1 loader cache, readable kernel procfs, protected
system images and the exact commands in [native execution](native-run.md#supported-contract).
Musl-only hosts are outside this GNU build target.

Linux compatibility completion is owner-accepted for this stage with focused
x64 checks and other behavior assumed from the shared Unix implementation.
Full package suites, live Claude/Codex sessions, additional distributions and
ARM64 build/runtime acceptance are not claimed. Signing and publication remain
separate distribution work; these scripts perform neither.

## Native Windows development

Use the pinned MSVC Rust toolchain and Visual Studio C++ build tools. From this
package directory in a Windows developer terminal:

```powershell
cargo build --locked
cargo test --locked --lib windows_opaque_shells_never_use_allow_fallback
```

Run strict native lint separately with
`cargo clippy --locked --all-targets -- -D warnings`.

The development binary is `target/debug/shield.exe`. Windows acceptance covers
AMD64 source builds, enrollment, setup/repair/removal, native console ownership,
private ACLs, MCP stdio, and Registry installation/offline reproduction. Claude
Code 2.1.312 file and PowerShell tools pass Allow/Deny/Ask and missing/timeout-hook checks with
an isolated local provider. Codex 0.166.0 loads the generated project hooks:
native command Allow/Deny, configured Ask allowance and prefix-covered Ask
blocking pass with retained Shield decisions. Unsupported PowerShell source
is also withheld by the generated hook.

To prepare an AMD64 release binary from the same developer terminal, run:

```powershell
powershell -NoProfile -File scripts/build-windows.ps1
```

For ARM64, install the Visual Studio C++ ARM64 tools and ARM64 Windows SDK
libraries, add the pinned Rust target, and use an MSVC developer terminal targeting
ARM64 (for example, the **x64_arm64 Cross Tools Command Prompt**):

```powershell
rustup target add --toolchain nightly-2026-04-16 aarch64-pc-windows-msvc
powershell -NoProfile -File scripts/build-windows.ps1 -Architecture arm64
```

An x64 Windows host can cross-build ARM64; it does not need to run the ARM64
executable. Native ARM64 development hosts can use an ARM64 developer terminal.
Both builds include MCP Kit, Registry Kit and TUI Kit. Process Kit is not used.

The outputs are `target/x86_64-pc-windows-msvc/release/shield.exe` and
`target/aarch64-pc-windows-msvc/release/shield.exe`, respectively. The checked-in
release configuration requests static C runtime linkage for Shield and its
dependencies. The script adds neutral source/cache diagnostic paths through a
private temporary Cargo configuration, then runs the `cargo build-windows` or
`cargo build-windows-arm64` alias. It checks the MSVC terminal's target architecture
and the resulting executable's PE architecture.
It refuses Rust flag overrides that would replace the verified build settings.
The explicit target keeps those flags away from host build
scripts and procedural macros. Inspect the executable's imported DLLs with
`dumpbin /DEPENDENTS` before distributing it; the development build uses the
toolchain's default runtime linkage.

Verify the built executable in a temporary installation with spaces in its path:

```powershell
$env:SHIELD_WINDOWS_RELEASE_BINARY = Join-Path (Get-Location) 'target\x86_64-pc-windows-msvc\release\shield.exe'
cargo test --locked --test integration windows_release_binary_runs_from_owned_installation -- --ignored
```

This checks startup, confirmed offline enrollment, replacement after exit and
removal while preserving the enrolled project. It verifies the portable binary;
package-manager installation and migration between released versions need their
own acceptance.

### Portable Windows packages

After building, optionally Authenticode-sign and timestamp `shield.exe`, then
package that final executable from the same checkout and toolchain:

```powershell
powershell -NoProfile -File scripts/package-windows.ps1 -Architecture x64
powershell -NoProfile -File scripts/package-windows.ps1 -Architecture arm64
```

Each command writes `dist/shield-<version>-<Rust target>.zip` and a `.sha256`
sidecar. `-OutputDirectory C:\Acme\shield-candidates` selects another directory;
existing outputs are refused. `CARGO_TARGET_DIR`, when set, selects the same
build directory for both scripts. The packager checks the binary architecture,
includes the product license, resolved normal/build dependency notices and Rust
standard-library notices, and indexes them in `BUILD.json`. Package metadata
describes a workspace candidate with no source-revision attestation; it is not
proof of a clean source checkout, a signature or native acceptance. Build again
after source or dependency changes before packaging.

The scripts do not sign or publish anything. Sign before packaging and compute
checksums after signing. ARM64 build/package execution has not been verified;
its implementation is accepted for this stage without native runtime acceptance.

To install a portable package, extract it into a stable folder you own and add
that folder to PATH. Check `shield --version`, then follow
[Your first Shield project](getting-started.md). Before an update, close Shield
and connected agents, replace the executable, run `shield mcp setup --repair`
for each project's agent registration, and restart the agent. After moving the
installation, repair those registrations as well. To remove it, run
`shield mcp remove` for each connected agent in each project, then delete the
installation folder. Preserve project `.shield` folders to keep policy/history.

### Supported Windows commands and acceptance

The PowerShell preview supports one literal invocation of `git`, `rg`, `node`,
`rustc`, `cargo`, `make`, `just` or `shield`. Arguments may be unquoted or use
single/double quotes without expansion. Empty arguments, embedded double quotes,
whitespace-containing arguments ending in a backslash,
compound commands, redirections, cmdlets, aliases and explicit shell overrides
are outside that grammar. Native handoff blocks unsupported PowerShell and cmd
source, including when Ask fallback allows ordinary policy Ask requests.
The [checked native runner](native-run.md) also supports three bounded System32
leaf forms on AMD64 and ARM64 Windows, requiring matching native PE images.
Native AMD64 acceptance covers Allow/Deny/Ask, exact
one-time grants, replay refusal, timeout, Ctrl-Break and console restoration.
Checked Git inspections support a protected Git for Windows `2.56.0.windows.3`
ucrt64 installation on AMD64, or clangarm64 installation on ARM64. Native AMD64
acceptance covers helper suppression, index preservation,
exact grants and refusal of wrappers, local copies and unsupported argv.
ARM64 checked execution uses the same ACL, held-image, grant and Job contracts;
it refuses foreign-architecture executables/DLLs, including x64 Git under
emulation. ARM64 runtime behavior is assumed working for this stage, not tested.
There are no published Windows installers or native ARM64 acceptance results.

The ignored native-host fixtures need Node 24+, Git for Windows and isolated
copies of Claude Code 2.1.312 and Codex 0.166.0. Install the pinned CLIs in an
owned directory outside the Windows temporary folder, then point
`SHIELD_WINDOWS_AGENTS_ROOT` at it:

```powershell
npm install --prefix C:\Acme\shield-agent-tests --no-save @anthropic-ai/claude-code@2.1.312 @openai/codex@0.166.0
$env:SHIELD_WINDOWS_AGENTS_ROOT = 'C:\Acme\shield-agent-tests'
cargo test --locked --test integration windows_agent_hosts_load_native_hooks_against_local_providers -- --ignored
cargo test --locked --test integration windows_agent_hosts_codex_loads_native_hooks_against_local_provider -- --ignored
```

Run these host checks separately. They use private ConPTY sessions, temporary
projects/profiles and scripted loopback providers. They make no external model
requests and do not change your ordinary agent profile. The public fixtures
include their provider implementation; no private SDK test package is needed.

With the protected Git version above installed in the default Program Files
location, run its isolated repository acceptance separately:

```powershell
cargo test --locked --test integration windows_git_inspections -- --ignored
```

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

`build.rs` checks the bundled skill's sources and calls `canonic-mcp-kit`'s
documentation build helper, which validates every guide under `docs/` and
writes one compressed bundle to `OUT_DIR`. An invalid guide, such as a symbolic
link or a page that is not UTF-8, fails the build with its relative path.

The `canonic-registry-kit` dependency handles registry identity, authentication,
acquisition, checkpoints, resolution, and UTC timestamps. Shield owns pack
declarations and the review that activates acquired artifacts. `canonic-mcp-kit`
handles framing, lifecycle, validation, cancellation, deadlines, and confirmation
requests; Shield's handler supplies its tools and limits. Its documentation
module also supplies the `shield_docs_*` tool definitions and queries;
`src/review/mcp/documentation.rs` embeds the bundle and maps the library's
failures into Shield's result envelope.

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
logging worker drains accepted frames with a 5 ms shutdown allowance. An unfinished
drain or dropped frames produce a diagnostic stderr warning. Slow storage does
not block live ping or cancellation dispatch. Frame sizes remain bounded by the
MCP input and output limits. Frame records include `observed_us`, captured when
the observer receives the frame, before worker parsing or file writes.

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

Request trace records add monotonic microsecond offsets, a wall-clock anchor,
connection/request/trace IDs and fixed stage names. `request_span` durations are
inclusive of child spans; do not sum nested durations into a request total.
`request_end` reports work counters, outcome and total handler duration. Counters
initialized to zero name instrumented work; absent counters have no reported
observation and must not be interpreted as zero.
`request_incomplete` means a request link was dropped without completion, not that
its mutation rolled back. Request records use a bounded nonblocking queue; loss
appears as `dropped_records`, `dropped_spans` or a diagnostic stderr warning.
Request-writer shutdown waits at most 5 ms before reporting an unfinished drain.
Internal callbacks retain their fixed hook-stage records and do not start the
asynchronous command-request tracer.

The apply trace covers validation, source hydration, policy preparation, cache
construction, writer-lock wait, publication, projection checks/repair and durable
sync operations. Legacy form continuations retain their request link and measure
the input or approval wait. Host permission obtained before Shield receives an
MCP call is outside this measurement. The handler boundary also excludes MCP
scheduler queueing, schema validation and final wire encoding; round-trip tests
measure transport overhead separately. Modern form retries have separate traces.
Proposal spans distinguish base capture, edits, next capture, comparison, plan
preparation, handle reservation and presentation. `policy.source_captures` counts
source validation independently of compiled-plan preparation, so re-parsing packs
cannot hide behind a zero preparation count.
Pack spans distinguish decoding, source validation, resolution and matcher
preparation. `pattern.compiled` and `pattern.reused` count newly built and exact
reused matcher programs. Rule/access-path proposal edits borrow matchers from the
preceding captured snapshot, while validating source and accounting the new
store's pattern bytes normally. Fresh compiled plans check their encoded layout
and retain source validation from typed construction, avoiding duplicate decoding
and semantic validation of just-encoded records. Untrusted full-image admission
remains covered separately by compiled-policy tests.
`handle.retain` and `source.identity` spans expose handle admission and canonical
candidate hashing. `handle.admission_encodings` counts new-handle size measurements;
cached handles skip them. `source.identity_encodings` counts candidate-identity
encodings, and `cursor.rows_copied` counts rows projected into a page. Shared JSON
size checks count encoded UTF-8 bytes without retaining an output string; source
hashing uses bounded scratch space and preserves existing compact-JSON digests.

Run the focused development regression with
`make -C packages/shield test-focus FILTER=apply_request_trace_explains_work CARGO_FLAGS='--success-output immediate'`.
It creates an isolated project, proposes and applies three batches through real
stdio, and prints timings and work counts. Optional
`APPLY_PROFILE_PROJECT=/path/to/enrolled-project` reads that project's immutable
pack artifacts into the temporary fixture. It does not copy settings, custom
rules, history or identity and does not write to the source project. This is a
local regression/profile fixture, not a cold-start or cross-platform latency
acceptance campaign.

`make -C packages/shield test-focus FILTER=ordinary_request_sweep CARGO_FLAGS='--release --success-output immediate'`
compares fresh and aged stores through retained MCP requests and the CLI inspection
owners. Optional `APPLY_PROFILE_PROJECT` selects immutable pack artifacts using
the read-only fixture described above. The test asserts source/preparation/history
work limits, including source capture counts, independent of unrelated retained proposals.
It prints inclusive stage timings as well as round trips. CLI-owner timings exclude
process startup and output; they are not full CLI latency measurements.

Production state readers select authority metadata, current source, one proposal
or optional history explicitly. Current policy readers may reuse its authenticated
compiled plan after checking source integrity; doctor forces source preparation
as a separate check. Full-source/history fixture readers are unavailable to
production code. CLI policy changes retain prepared evidence across review and
commit, with fresh authority and file checks at publication.

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
These SDK and interactive host fixtures are verified on macOS. The separate
Windows native fixtures and their limits are described
[above](#native-windows-development). Linux uses focused platform checks and
owner-accepted shared behavior [described above](#native-linux-development).

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
