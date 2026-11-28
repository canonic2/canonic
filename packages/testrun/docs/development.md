# Testrun development

Run Cargo from this package, where rustup selects the pinned toolchain. A
focused run is `cargo test --locked --lib <filter> -- --test-threads=4`; replace
`--lib` with `--test diagnostic_analysis` or `--test integration` to select an
executable test binary. The filters below name the fixtures for each area.

The `diagnostic_scenario_context` library tests verify lifecycle/media/
energy/setting schemas, clock and identity boundaries, native Android read plans/
filtering, restoration outcomes and private page bounds. The same filter with
`--test diagnostic_analysis` verifies real CLI/parser/manager and source-deletion
replay with one pinned binary.
These fixtures do not certify live hardware/peripheral/audio-route or restoration.

The `diagnostic_network_data` library tests verify HAR phases/context,
scoped IDs, protocol/retry/message records, bounded PCAP headers, scalar indexes,
read-plan budgets, snapshot differences and private page bounds. With
`--test diagnostic_analysis` the filter exercises public CLI/manager/isolated
parsing and source-deletion replay. Existing proxy/HTTP/storage acceptance
retains its own scope; these fixtures do not certify live databases, remote
cost enforcement or native packet capture.

The `diagnostic_performance` library tests verify startup/interaction clock
and uncertainty boundaries, per-frame deadlines, native FrameTimeline separation,
tree identity/candidates, package-size compatibility and bounded pages. With
`--test diagnostic_analysis` the filter exercises the real CLI/manager/parser
and replay after input deletion. Use the `diagnostic_native_trace` filter for
the changed Perfetto source/config and converter lifecycle fixtures. These
tests do not certify live display/GPU timings, framework instrumentation or
accessibility conformance.

The `diagnostic_memory` library tests verify HPROF/producer graph models,
independent dominator-oracle cases, paginated references, resource series and
Apple memory export/opt-in boundaries. Select `--test diagnostic_analysis`
with the same filter for real CLI heap queries, retained replay and probe-digest
admission. These checks do not certify live runtime heaps or memory instrumentation.

The `diagnostic_native_trace` library tests verify native trace admission,
owned capture/converter peers, scheduling/sample/XML interpretation and cleanup.
With `--test diagnostic_analysis` the filter verifies the real executable's
retained queries and native-artifact export.
These fixtures do not profile a live app or certify a device/template/tool range.

`src/protocol.rs` drives canonic-mcp-kit's I/O-free Session and validated Catalog.
Testrun's Tokio transport owns reads/writes, lanes, deadlines and cleanup;
`src/schema.rs` maps shared argument errors to Testrun errors. The library's stdio
driver is not used. `tests/integration/mcp.rs` exercises strict startup, frames
and catalog publication; scheduling fixtures cover cancellation, fatal duplicate
IDs, task stop priority and native probe disposal. See [MCP transport](mcp.md).

The bundled guides use canonic-mcp-kit's documentation module. `build.rs` calls
its build helper, which bundles every Markdown page under `docs/` into
`OUT_DIR`, refuses symlinks and asks Cargo to rebuild when `docs/` changes; the
selected pages also feed the runtime build identity. `src/documentation.rs`
embeds the bundle, publishes the library's three tools, runs each query on a
blocking thread with a deadline and cancellation check, and maps the library's
failures to Testrun errors. `protocol/responsive.rs` admits these calls without
runtime locks or manager calls.
`documentation/tests.rs` covers byte-exact paginated reads, error mapping and
cancellation reaching the blocking query; protocol fixtures cover schemas,
envelopes, cancellation and replies while a mutation is running, and startup
fixtures exercise docs with absent host tools and unusable runtime storage.

The optional RN app probe lives in `probes/react-native.js`; its exact CommonJS
template is embedded in `diagnostics_capabilities.reactNativeProbe.source` and
included in the build identity. `node --test tools/react-native-probe.test.mjs`
verifies the helper without React or a device. The `diagnostic_react_native`
filter runs owned inspector/transport/settlement fixtures; use
`--test diagnostic_analysis` for executable parser/template checks.
These fixtures do not certify a live RN/Expo build/version matrix.

The `diagnostic_dart` filter exercises owned VM Service endpoints,
credential handling, lifecycle and inspector cleanup. Select
`--test diagnostic_analysis` for real parser and retained CLI
snapshot/size/symbol-reference checks. The optional `probes/dart.dart` template
ships through capability discovery and is included in the build identity; it
requires validation with an app's Dart SDK. Fixture checks do not certify template
execution or a live SDK/device/build range; the dated research records host limits.

`cli/entry.rs` owns argument diagnostics, help handling and exit policy in the
library. `main.rs` creates the Tokio runtime and applies the MCP-only shutdown
boundary after the CLI returns. Unit tests cover parsing and exit decisions;
startup fixtures verify their process output and status.

`src/playback` owns managed media validation, bounded signal statistics and private
clip artifacts, plus controller-owned Android/Indigo
adapters. `qa/playback.rs` uses the existing session leases and outcome archive;
`managed/playback.rs` adds durable receipts and status lookup. The Android helper
source is `helper/android/io/canonic/testrun/media/`; Cargo compiles it into DEX
and embeds it in the binary. No helper sidecar or runtime Java installation is
needed by local/project installers.

Building requires JDK 17+, Android SDK Platform 36 and Build Tools 36.0.0. Set
`ANDROID_HOME` or `ANDROID_SDK_ROOT`; default discovery uses
`~/Library/Android/sdk` on macOS or `~/Android/Sdk` on other hosts. `JAVA_HOME`
selects javac when set. Build output stays in Cargo's `OUT_DIR`. The initial
device helper requires Android 12/API 31 or newer and verifies shell authority
and framework service initialization before readiness.

`tests/integration/playback.rs` exercises published MCP/CLI media calls against
an owned adb/helper protocol fixture, including duplicate tags, preflight,
deduplication, unknown outcomes, UI coexistence and device-side helper cleanup.
`playback/ios.rs` tests balanced consumer reports and cancellation/failure release
without hardware. These host fixtures do not establish real player/device support.

`src/applications.rs` owns durable deployment transactions;
`applications/package.rs` owns bounded private package snapshots;
`applications/adapter.rs` owns Apple/Android installation and inventory commands;
`applications/physical.rs` owns bounded read-only USB installation-proxy inventory.
`app_lifecycle/termination.rs` owns physical app termination by installed
executable/process identity and native absence verification, independently of XCTest.
`managed/applications.rs` reuses the target's connected device actor or, on a
target that is not connected, takes temporary exclusive leases for one call
through the target's queue. `tests/integration/applications.rs` exercises the
CLI and MCP using unique fake adb identities, including app commands before
`connect` and through a connected device shared by several clients. Platform-command fixtures do not establish real deployment,
data-retention or developer-image/media coexistence acceptance.

`configuration/` owns strict YAML layers and field provenance. `builds/` owns
builder plans, retained operations, bounded artifact processing and stable
submission recovery; `managed/service/builds.rs` composes connection, install
and launch as device stages queued on the target like any client's requests. Build unit fixtures exercise recovery,
cancellation, timeout/log caps and package handling; app integration fixtures
exercise CLI and MCP composition with fake Android tools.

Opt-in native build acceptance uses the `acceptance-builds` feature. Each case
runs alone as `cargo test --locked --features acceptance-builds --test integration <case> -- --nocapture --test-threads=1`
with the settings below in its environment:

| Case | Settings | Scope and prerequisites |
| --- | --- | --- |
| `native_owned_xcode_debug_release` | none | Owned UIKit project, Debug and Release simulator builds and identity inspection; macOS/Xcode; no simulator boot or installation |
| `native_owned_xcode_debug_release` | `TESTRUN_BUILD_DEPLOY=1` | Owned UIKit Debug/Release and custom-package build/install/launch on a newly created simulator in an isolated set; checks launch env/args and retained app data; deletes its simulator afterward |
| `native_owned_gradle_debug_release` | `TESTRUN_TEST_GRADLE=/path/to/gradle TESTRUN_TEST_ANDROID_SDK=/path/to/sdk` | Owned Android project, Debug/Release signed APKs and identity inspection; cached Gradle 9.3.1, AGP 9.1.0 dependencies, JDK 17+, SDK/Build Tools 36; offline, no emulator startup |
| `native_owned_gradle_debug_release` | the same plus `TESTRUN_BUILD_DEPLOY=1 ANDROID_HOME=/path/to/sdk` | Same Android builds on a newly created isolated AVD booted from the selected SDK's emulator; requires API 36 Google APIs Play Store x86_64 image; checks the configured app UI and data retained by replacement |
| `selected_native_build_profiles` | `TESTRUN_BUILD_PROJECT=/path/to/acme TESTRUN_BUILD_PROFILES=ios-debug,ios-release` | Explicit existing project and prepared framework dependencies; selected profiles build without device access |

Adding `TESTRUN_BUILD_TARGET=EXPLICIT_TARGET_ID` to `selected_native_build_profiles` selects
build/install/launch with replacement enabled. Use only an explicitly authorized
target and an owned app: replacement can change installed app data. The runner
disconnects the target after success. Supply separate prepared React Native,
Expo/EAS or Flutter projects to verify those framework/toolchain combinations;
native Xcode/Gradle acceptance does not establish framework acceptance. Failed
native CLI builds retain their result and available private build log in a
temporary evidence directory reported by the test.
The offline Gradle fixture forwards the selected SDK's Build Tools 36 `aapt2`
through an explicit Gradle property; its other plugin dependencies must be cached.
Set `TESTRUN_TEST_BUILD_NETWORK=1` explicitly to permit Gradle dependency downloads
for that fixture. The default remains offline.

For real framework fixtures, explicitly prepare a temporary project with
`node tools/bootstrap-build-project.mjs flutter /private/tmp/testrun-build-flutter`
(or `react-native`/`expo` with a distinct root). The helper downloads framework
dependencies, records versions, creates neutral app profiles and preserves its
owned fixture directory for reuse. Flutter includes its own temporary SDK and
package cache. React Native iOS also needs CocoaPods preparation in the generated
`ios` directory. Expo native profiles require generating the native Android
project with `pnpm exec expo prebuild --platform android --no-install` first.
EAS local builds still require an authenticated Expo project; the fixture uses
an isolated home and does not adopt host account credentials.

`test-build-native` accepts `TESTRUN_BUILD_TARGET=disposable:ios` or
`disposable:android` to create and prepare an isolated target, run the selected
profiles with installation/launch, and release its resources. Android requires
`ANDROID_HOME` and the image above. This is live acceptance and requires explicit
authorization for the disposable target scope. Set `TESTRUN_TEST_BINARY` to an
absolute installed executable to run these build fixtures against that binary.
The default flow composes build/install/launch for each profile. Set
`TESTRUN_BUILD_FLOW=separate` to compile all artifacts first, then prepare the
target and exercise independent artifact installation and app launch. That mode
launches by app ID; use the composed mode to verify profile launch args/env.
Both deployment modes require the launched app to become foreground. The Android
fixture uses a 720×1280 display; product deadlines are unchanged.

`src/device_setup.rs` owns bounded readiness and Developer Mode guidance;
`device_setup/discovery.rs` merges safe Apple device metadata;
`device_setup/support.rs` owns exclusive Cryptex replacement and service checks;
`device_setup/support/assets.rs` owns pinned acquisition and cache verification.
`host_setup.rs` owns shared signing selection; `host_setup/identities.rs` reads
certificate-derived teams, and `host_setup/storage.rs` owns locked per-user
settings transactions. `cli/setup.rs` owns terminal prompts
and `signing setup` presentation. `mcp setup` composes `agent_setup.rs`, whose
`agent_setup/` modules own host codecs, ownership, review and recovery; it does
not contact the manager.
`device/signing.rs` owns provisioning rules.
`device/cache.rs` owns verified signed-runner reuse and its
per-key lifetime lease. Focused fixtures cover certificate-derived team IDs,
saved choices, profile expiry/authorization and cache reuse/invalidation.
`android/readiness.rs` owns bounded Android doctor checks and partial observation
readiness reports, shared by direct CLI/MCP diagnosis and probed listings.
`tests/integration/startup.rs` checks finite-stdio startup, the published catalog
and managed Android routing through a fake adb. These fixtures do not certify
live image replacement or phone reboot recovery. The explicit developer-support
download suite verifies real public assets without contacting a phone.

`managed/service/lifetime.rs` owns device release on `disconnect`, idle release,
automatic release retry and manager exit. `managed/lifetime.rs` settles durable
cleanup records whose device actor or manager is gone and builds the cleanup
inventory that status, blockers and recovery report. Host task end
(`testrun task stop`, MCP `testrun/task_end`) releases nothing; the MCP server
refuses requests it received before the task end with `TASK_ENDED`.
`tests/integration/android_contract/host_lifecycle.rs` exercises task end and
client departure leaving devices connected, task end refusing queued input while
dispatched input settles, manager loss, automatic retry of a failed release,
foreign records reported as blockers, manager lifetime and build handover
through the shipping CLI/MCP with the owned adb fixture.
Build compatibility uses a Cargo-generated runtime fingerprint; changing a
compiled input or build configuration hands the device over to a new device
actor on the next `connect`. Integrating an agent hook requires verifying that
its event means permanent task completion, rather than turn completion; these tests do not certify any agent's hook API.

`build.rs` fingerprints all bundled Markdown and expected Rust, native, Java
and runner source types, including the runner project and shared scheme. Local path dependencies, found
transitively from each `Cargo.toml` excluding dev-dependencies, contribute their
manifest, any build script and `src/` Rust files, since `Cargo.lock` records no
checksum for them. Dotfiles, editor backups and
generated runner directories are excluded. Source directory watches detect new
and deleted inputs; a Rust edit reruns the fingerprint but reuses successful
Android DEX and native archive outputs in `OUT_DIR` while their own source and
tool configuration remain unchanged. Failed builds invalidate that artifact's
cache record before invoking its compiler.

`device_media/startup.rs` retains physical acquisition tasks, the device-media
lease and remote release witnesses when their caller is cancelled. The registry
fixtures exercise cancellation before readiness, background release and retry of
an unverified release. `tests/integration/android_contract/recovery.rs` exercises
startup task end, controller death after input dispatch and disconnect/reconnect
ordering through shipping MCP/CLI processes with an owned adb fixture.

## Fast iteration

From this package directory:

```sh
cargo build --locked
cargo test --locked --lib 'protocol::' -- --test-threads=4
# Select integration tests explicitly when needed:
cargo test --locked --test integration 'selected_test_name' -- --test-threads=4
# Repeat a focused set with installed cargo-nextest, preserving its normal concurrency:
cargo nextest run --locked --test diagnostic_analysis --stress-count 3 --no-fail-fast --test-threads=4 diagnostic
```

`cargo build` uses Cargo's incremental development build, writing
`target/debug/testrun`. It does not install a command or build a release. Pass
`--lib` with a substring filter to avoid unrelated integration and binary test
targets; four test threads is the host limit shared with other work.
`cargo nextest run --stress-count` repeats a focused set with no fail-fast;
cargo-nextest must already be installed, and ordinary focused runs do not
require it. `.cargo/config.toml` raises `RUST_MIN_STACK` for every test, since
unoptimized MCP request futures outgrow the default test-thread stack. Keep complete failure output in a temporary log.
Use a real test name in the integration example; Cargo reports how many tests matched.
Keep this package's `target/` directory between iterations; Cargo checks source
freshness on every run. Dependency, toolchain, profile and feature changes may
require a longer rebuild. Run the development executable directly to try edits. Use release builds
for performance measurements. Select completion/release verification categories
individually. Existing build, installation and acceptance commands retain their
behavior.

Build the single binary with `cargo build --release --locked`. Rust 1.102 or later
is required; `rust-toolchain.toml` pins the development toolchain to nightly-2026-11-09.
canonic-tui-kit is a required dependency for CLI interaction. Signing choices use
its shared inline picker; MCP and unattended requests never acquire a terminal.
CLI presentation maps supplied facts to shared settled-document blocks; human
output is the default and public CLI fixtures that parse results pass `--json`.
See the [CLI guide](cli.md) for output and cancellation behavior.
Native simulator code links Apple's macOS media frameworks and
loads the selected Xcode simulator frameworks at runtime.

### Local installation and release candidates

Run `cargo xtask install` from `packages/testrun` to install the command into
`~/.local/bin`. The task uses locked Cargo dependencies. Pass
`--prefix /path/to/prefix` to choose another root and put its `bin` directory on
PATH. The installer puts runner source and the license in `PREFIX/share/testrun`,
where optional XCTest setup can use them without the checkout. See
[distribution](distribution.md) for host requirements and manager replacement.

The local installer builds for the host. Canonic's portable GNU/Linux candidates
use an Ubuntu 22.04 build environment and are checked against glibc 2.35 for the
selected architecture. The archive includes `BUILD.json`, and the Linux
verification sidecar records the package digest and CLI/MCP startup checks.
The shared release driver can also sign the verified archive with Cosign and
write a detached `.sigstore.json` bundle.

```sh
cargo fmt --all --check
cargo clippy --workspace --all-targets
cargo test --all-targets -- --test-threads=4
bash tests/concrete-tap.sh # macOS; no device needed
bash tests/runner-transport.sh # macOS; owned loopback sockets, no device
bash tests/simulator-geometry.sh # macOS; no simulator needed
cargo test --doc
cargo doc --no-deps --document-private-items
```

Pure tests cover input/schema validation, framing, decoder recovery, viewer
credentials, queries, generation changes, process bounds and atomic artifacts.
Boundary regressions cover editable-focus detection across platform tree shapes,
inclusive gesture-duration bounds and capped image reads that preserve evidence
metadata when attachments are unavailable or oversized.
HID tests exercise paired release, ordered swipe contacts, keyboard pressed sets,
modifier ordering, lazy service ownership/removal, cancellation and failed
key-release/removal quarantine without a device. A synthetic HEVC frame exercises the
native live decoder and calibrated padding crop. Physical testing defaults to
HID/video; use the explicit XCTest path to verify full-quality evidence and AX.
Recovery tests use real private sockets and kernel locks: closing status remains
queryable, removed/replaced endpoints preserve foreign files, an owned killed
subprocess releases both leases, and verified media release frees exclusion while
retaining stream diagnostics. Unverified release retains ownership for recheck.
Native decoding also exercises completion after the final
encoded-frame notification, with no later frame to wake the observation waiter.
The source wait fixture preconstructs its real native decoder before timing
callbacks; native codec coverage separately checks a lone submission and a
129-frame sequence, with no later frame required for completion. Application
cancellation uses an independently scheduled owned task and awaits its abort
before checking the durable unknown outcome.
Observation-worker regressions retain a predictive startup burst while decoder
creation is blocked, enforce the compressed-byte budget including the in-flight
frame, and release reservations during saturation recovery and shutdown.
Device-media tests also cover bounded HEVC assembly under packet loss, sequence
wrap, malformed aggregation and oversized frames; a synthetic compressed fixture
checks native MP4 muxing and full-track decoding without capture permissions.
The default Cargo suite runs host fixtures; live platform and asset-download
coverage is selected through the explicit acceptance suites below.
Simulator-source process fixtures use `python3` on the host to exercise bounded
IPC, concurrent consumers, canceled startup waiters and retained cleanup without
opening a simulator.
Managed-workflow fixtures use separate owned controller processes and real Unix
sockets. They cover endpoint removal, interrupted replies, close/reacquire,
manager termination, failed cleanup, foreign ownership and stale disconnects.
MCP tests call the published five-operation workflow and verify request deduplication and observation recovery
through a nine-second owned cleanup. CLI and MCP use the same operation budgets.
Resumable-disconnect regressions inject failed evidence publication and a
failed nonce-checked helper release. They remove the fault, retry disconnect and
check unchanged completed action history, retained release errors, read-only
status and single input/quit dispatch. Media-registry fixtures retain a transient
remote release failure, then verify release without restarting the source or
touching a foreign kernel lease. Socket-loss recovery also checks missing and
foreign replacement endpoints, stale-epoch requests, durable failed status and
MCP failure followed by CLI release; status and consumed markers never retry.
The owned-socket fixture also sends a fenced malformed close, checks a validation
reply with unchanged ready ownership/connectivity, then observes and disconnects
through MCP. Refused close requests cannot initiate cleanup.
Terminal worker errors remain failures.
Video receive-error fixtures distinguish stream health from verified release:
successful teardown retains `progress.diagnostics.device_media.streamError`
without failing disconnect or retaining a released source/lease. The registry
transfers this diagnostic into session cleanup history after source removal.
Selected session, preview/recording and startup cleanup only releases that
device's unused sources. A real source-task panic fixture retains its UUID
verifier, refuses removal while remote release is unverified and succeeds after
a later absence check. An independent-run fixture publishes completed evidence
while the failed session retains its own run and kernel control lease.
`Server::shutdown()` is best-effort cleanup for MCP EOF/termination and manager
exit: it attempts every global owner despite session errors. Foreground
`session` shutdown uses the same path and exits on cleanup failure, including
after SIGINT/SIGTERM; the shipping CLI/adb fixture verifies both signals.
It first fails an explicit close, then terminates the still-running foreground
controller. The stdio fixture fails a session's recording pull before EOF and
checks that process exit still publishes an independent run's summary while
leaving the failed run unsuccessful.
An explicit foreground close failure retains the endpoint for failed status,
journal lookup and close retry, including when only run-summary publication
remains. Observations and mutations are refused until cleanup completes. The
shipping fixture preserves the original error across repeated failed closes,
then repairs its evidence destination and retries successfully.
Device actors use `shutdown_resumable()` for a release that will be retried, so
a failed release keeps its shared registries for the automatic retry. Real media-broker fixtures distinguish these
paths and verify that both retain cleanup failures.
Transport regressions distinguish host socket denial from lock/disconnection and
transient failures. An existing foreign endpoint is preserved; failed local
startup closes without contacting a fake physical target. A retryable acquisition
failure is retried once without input.
These host fixtures replace
the native controller boundary; they do not establish physical-device delivery.
CLI commands and MCP servers are clients (`src/managed/remote.rs`) of one
manager per user and data root (`src/managed/service.rs`, started on demand
through the hidden `testrun managed-owner --root` entry point). The manager
runs on `canonic-process-kit` (`src/managed/host.rs`,
`src/managed/remote/manager.rs`): the library owns the data root's lease and
private sockets, peer checks, framing, detached startup, admission,
retirement, drain and MCP attachments as handed-off connections. Each client sends
its identity with every request (`src/managed/identity.rs`): an optional host
label from `TESTRUN_TASK_ID` or `CODEX_THREAD_ID`, the workspace that namespaces
test builds, and `TESTRUN_SIMULATOR_DEVICE_SET`. Identity grants nothing; every
client reaches the same devices. An MCP server keeps one connection open, so the
manager stays running while the server does.
The manager's device pool (`src/managed/service/registry.rs`) holds every
connected target with its last use on the awake clock (`src/managed/clock.rs`),
temporary claims of app operations and preparation, a viewer hold and a failed
release with its retry backoff; nothing in it belongs to a client.
`src/managed/service/requests.rs` assigns request roles and resolves each request
to a pooled target before admitting it to that target's FIFO queue, which every
client's requests share. `src/managed/service/lifetime.rs` releases a target on
`disconnect` or after ten minutes without use, retries a failed release two
seconds later and then at doubling intervals up to 30 seconds, and keeps one
library hold while anything is pooled; the library exits the manager once
nothing is left. `src/managed/service/recovery.rs` serves the
control subset behind `testrun manager status|blockers|retry|operation show|stop|drain`.
Each device actor runs on its own thread under a supervisor
(`src/managed/supervisor.rs`) that holds the device's lease and native handles.
The data root is `TESTRUN_DATA_ROOT`, else the user-specific
`/tmp/testrun-qa-<uid>/`; it holds the manager's lease anchor
(`manager.lock`), its ordinary and control sockets (`manager.sock`,
`control.sock`), its log (`owner.log`) and record (`owner.json`), and a
`v7-<hash>` pool directory with each target's selection, action and device
actor records.
Tests and development give each run its own data root, so its manager
inherits that run's fake tools; integration fixtures use `Workspace` in
`tests/integration/main.rs`. Controller/media authority is verified by a
kernel-held ownership generation, not by the recorded PID. Records survive
disconnect and have no automatic expiry; status returns at most eight history
entries. Manager logs and diagnostics remain private and are not required for the
QA workflow. A client that reaches a manager of another protocol asks it to
retire when empty or to drain otherwise; new clients receive
`OWNER_UPGRADE_PENDING` until it retires. `src/managed/transition.rs` settles
records of the current layout that a lost manager left and reports what it
cannot settle as blockers; records of earlier layouts are kept as history.
Target leases are shared by every manager and data root, so
a device held elsewhere is refused with `DEVICE_IN_USE`.
Unit tests drive the manager against fixture device actors
(`src/managed/tests.rs`, `src/managed/service/tests.rs`): clients in different
workspaces sharing one device's FIFO queue, target resolution, release and its
automatic retry, idle release, quarantine, upgrades and exit. Upgrade,
polling and attachment cases serve the fixture through the library on the
test's own data root (`host::fixture`). Pool decisions
have their own unit tests in `src/managed/service/registry/tests.rs`.
Batch tests also cover whole-sequence preflight, stop-on-error, retained/conflicting
command IDs and refusal after uncertain dispatch. Private IPC tests exercise
reconnecting clients, peer identity, message bounds and endpoint ownership.
The owned simulator input case checks two native batch taps, duplicate suppression
and the final image in the MCP response.
The managed simulator case also checks eight-action per-step and final-only image
results, independently rendered counts and durable results after disconnect.

## Source ownership

`src/browser/` owns the private browser binding: `transport.rs` frames bounded
JSON, `schema.rs` validates generated protocol data, `objects.rs` owns epoch-bound
references, `dispatcher.rs` applies lifecycle events and correlates calls, and
`connection.rs` / `events.rs` own independent bounded I/O/control/event lanes.
Run in-memory fixtures with `cargo test --locked --lib browser:: -- --test-threads=4`.
That binding carries no session authority. `contract.rs` owns backend-independent
lifecycle requests, schemas and initial capability/version pins. `lifecycle.rs`
owns resource kinds and generations; `session.rs` and `session/` supervise owned
Firefox/WebKit launch, context/page selection, handle/document lifetimes, callback
expiry and retained operations. `journal.rs` publishes private immutable outcome
records; it never records request expressions or prompt values. `preparation.rs`
and `preparation/` own pinned downloads, safe extraction, component locks and
installed-tree verification. `runtime.rs` owns browser resources inside the
manager; `surface.rs` supplies the seven public lifecycle definitions.
`browser/api.rs` extends the shared catalog with explicit browser variants and
translates their action vocabulary into the same retained session workflows.
`session.rs` owns ordered browser batch admission, partial results and cancellation.
`managed/browsers.rs`, `cli/browser.rs` and QA dispatch compose the same manager pool for
CLI and MCP; each browser connection is a `browser:<connectionId>` target. Browser operations keep their own mutation/response lanes, while
public status and callback submission stay responsive through MCP.

`browser/cdp.rs` and `browser/cdp/` own the independent native Chrome transport
foundation: bounded WebSocket upgrade over an admitted stream, JSON envelopes,
flattened-session reply checks, separate ordinary/control admission, retained
late replies and bounded events. `cdp/schema.rs` validates the pinned native
protocol; `bootstrap.rs` checks exact selected build identity before discovery.
`resources.rs` and `resources/events.rs` own context/target/session/frame/world
identity and document replacement independently of Playwright. `cdp/process.rs`
owns the isolated Chrome process/profile and `cdp/session/` owns retained native
lifecycle workflows and event/reply ordering. `browser/owned.rs` selects that
backend or the Playwright session behind the manager.
In-memory fixtures use `cargo test --locked --lib browser::cdp:: -- --test-threads=4`.
Private Chrome preparation pins the archive size/digest and installs no Node or
Playwright runtime. Public Chrome operations include lifecycle, navigation,
bounded reading/snapshots, conditions, frames, native input/actions, dialogs,
page events, registered file/directory transfers, capture/comparison, JPEG
preview, native H.264 recording and viewer control. Broader diagnostic/state
workflows remain incomplete.

`browser/reading.rs` and `reading/factory.js` compose the pinned browser-only
selector/ARIA helper, shared by native Chrome and Firefox/WebKit's safe ARIA
reader. Secure input values are masked in the generated accessibility tree
without modifying DOM inputs. `node --test tools/browser-cdp-helper.test.mjs`
tests extraction refusal and string decoding. In a combined Canonic checkout,
`node tools/browser-cdp-helper.mjs /path/to/playwright-core` extracts literal
helper bytes from the verified package; adding `--check` checks drift.
The [selector manifest](../tools/browser-provenance/cdp-selector.json) records
its source, helper, adapter and Mac editing-command hashes. The shared bounded
DOM reader, document-local element registry and condition observations live
under `browser/reading/`.

HTTP multipart filename inference uses the pinned MIME table without requiring
Node at runtime. `node tools/browser-http-mime.mjs /path/to/playwright-core`
extracts the table and its MIT license as data; adding `--check` detects drift
and `node --test tools/browser-http-mime.test.mjs` checks extraction refusal and
alias handling.
The [MIME manifest](../tools/browser-provenance/http-mime.json) records the
source and generated-data hashes.

`TESTRUN_BROWSER_ROOT=/absolute/private/cache cargo test --locked --features acceptance-browser-lifecycle --lib item_two_shared_application -- --test-threads=1`
runs the common application journey on test-owned Chrome, Firefox and WebKit in
headed/headless modes. It requires explicit browser acceptance scope. Individual
test suffixes select one engine/mode. The separate installed public checkpoint
selects `installed_items_two_three_cli_mcp_checkpoint` with
`--features acceptance-browser-public --test integration` and
`TESTRUN_TEST_INSTALLED_BINARY=/absolute/path/to/testrun`. Both fixtures join
their own resources and never attach to regular browser profiles. Clipboard
origin-refusal checks do not read or overwrite the system clipboard.

The same focused command with the `item_three_capture` filter exercises all six
owned capture/media cells, including CSS/device pixels at DPR 1/2, odd viewport
padding, masks and comparisons, annotations, headless Chrome PDF, authenticated
viewer input, a stalled frame viewer, recording independence and source recovery
after viewport growth/document replacement. Each cell catches assertions and
disconnects its owned resources before propagating a failure.

`TESTRUN_BROWSER_ROOT=/absolute/private/cache cargo test --locked --features acceptance-browser-lifecycle --lib browser::cdp::session::native -- --test-threads=1`
is an explicit live acceptance suite. It prepares only the pinned private Chrome
build, launches two isolated connections in each headed/headless mode, checks
reference isolation and context/page cleanup, and verifies profile release.
Select its host/resource scope before running it. The suite does not establish
installed CLI/MCP acceptance or escaped-descendant cleanup.

In a combined Canonic checkout, `node tools/browser-parity.mjs target/debug/testrun`
regenerates the internal comparison inventory, embedded native CDP schema and
BSD notice. It uses the debug binary from `cargo build --locked` for the public
catalog and fetches upstream source data without executing it. Run
`node --test tools/browser-parity-extract.test.mjs` in the same checkout to test
the comparison extractor and delivery mappings.

`node --test tools/browser-rust-surface.test.mjs tools/browser-public-surface.test.mjs`
checks source extraction without network access or upstream code execution.

`node --test tools/browser-page-reliability.test.mjs` checks that a click can
complete when its target removes itself and that oversized page snapshots stay
within the requested budget while retaining usable locators. Run it from
`packages/testrun/`.

The native Firefox/WebKit suites each run alone with
`TESTRUN_TEST_NODE`, `TESTRUN_TEST_PLAYWRIGHT_CORE` and `TESTRUN_TEST_BROWSERS`
naming the selected absolute Node executable, pinned package directory and
private browser cache:

| Suite | Command |
| --- | --- |
| Lifecycle, headed and headless | `cargo test --locked --features acceptance-browser-lifecycle --lib browser::session::native -- --test-threads=1` |
| Page controls and screenshots | `cargo test --locked --features acceptance-browser-lifecycle --lib browser::session::page_native -- --test-threads=1` |
| Uploads, choosers and downloads | `cargo test --locked --features acceptance-browser-lifecycle --lib browser::session::transfer_native -- --test-threads=1` |
| Context settings, permissions and media | `cargo test --locked --features acceptance-browser-lifecycle --lib browser::session::settings_native -- --test-threads=1` |
| Virtual time | `cargo test --locked --features acceptance-browser-lifecycle --lib browser::session::clock_native -- --test-threads=1` |
| Preview and recording | `cargo test --locked --features acceptance-browser-lifecycle --lib browser::session::preview_native -- --test-threads=1` |
| Driver fixtures | `cargo test --locked --features acceptance-browser-driver --lib browser:: -- --test-threads=4` |

`browser/page.rs` defines page/locator models; `session/page.rs` implements reads,
actions and waits with event barriers. `session/capture.rs` validates and publishes
dedicated screenshots. `browser/transfer.rs` and `session/transfer.rs` own cleared
fixture records, intercepted choosers and retained bounded downloads. The native
page and transfer suites use the same three explicit private paths as the
lifecycle suite; each runs Firefox and
WebKit headed/headless against synthetic local content and verifies cleanup.

`browser/input.rs` owns native action validation, short-lived CSS viewport
observations and held-state accounting; `session/input.rs` dispatches retained
native phases and balances failed/cancelled input. `keyboard-layout.json` is
generated from the pinned driver by the inventory entry point; `keyboard.rs`
uses its physical identities to balance aliases. Geometry listeners renew when
the document root changes. `test-browser-page` includes native keyboard, mouse,
touch and screenshot-observation cases; session pipe fixtures cover release
ordering and unknown-release no-replay behavior.

`browser/settings.rs` owns context setting, permission/origin and media option
validation. `session/settings.rs` applies environment mutations and reports
native outcomes separately from observed page effects. The settings suite
uses the three selected private paths and owned localhost fixtures for context
isolation, grants/geolocation, media restoration and concurrent recording.
`browser/clock.rs` defines typed virtual-time requests. Context initialization
preserves native paint scheduling in `session/page/native-timing.js`; preview
barriers use it independently of the application's Clock. The virtual-time
suite checks virtual timers, context scope, input observation age and media while paused.

`browser/frames.rs` separates bounded JPEG delivery from lifecycle events;
`browser/preview.rs` owns preview references/options and `session/preview.rs`
owns native source start/stop, geometry barriers, restart and private viewer
publication. `media/external.rs` adapts that feed to the existing native-host
grant and WebSocket service. The preview suite uses the same three
explicit private paths and covers source epochs, decoded corners, odd resize,
viewport growth, navigation, independent recording and repeated stop in both
engines/modes. `browser/recording.rs` owns bounded native JPEG-to-H.264 encoding;
`session/recording.rs` owns retained jobs and source consumers. The focused native
recording fixture checks full-track decoding, static duration, source-loss partials
and decoded quadrant colors/orientation. Native media fixtures and performance,
fault and installed-release gates retain separate scopes.

`driver.rs` owns the explicitly selected diagnostic Node/driver process, its private
stderr and forced group termination. It does not acquire dependencies or finalize
browser/media resources. Its raw termination API is a final process step after
workflow admission and owned resources have been closed. The lifecycle adapter
uses orderly `shutdown` instead: close the owned pipe, wait for upstream browser
finalization and reap the driver, drain stderr, and verify macOS descendant
birth-time witnesses in `processes.rs`. Unknown process inspection or incomplete
cleanup retains ownership for retry. Native Chrome CDP has its own transport
module and is not implemented by these Playwright process APIs.

`node --test tools/browser-inventory.test.mjs` runs the browser inventory parser
fixtures with Node. In a combined Canonic checkout, run
`node tools/browser-inventory.mjs /path/to/playwright-core` to regenerate the
internal source inventory and `src/browser/protocol-schema.json`. Add `--check`
to detect drift.

MCP target runtimes own their session/native state and ordered mutation queues.
Each target admits eight operations; independent screenshot, accessibility,
inventory and clipboard reads have four session slots. Prepared Android media
reads also use independent handles. Status/ping/tool listing have eight reserved
slots. Discovery and selected managed reads have a separate four-slot pool;
target work has a 64-job connection bound. Pool saturation reports JSON-RPC
`-32000`, or `OVERLOADED` from manager/controller/target admission, before affected
work dispatches. An explicit MCP execution timeout starts at dispatch from the
transport lane. Managed device-operation budgets start after the target queue
for MCP and include that wait for CLI.

CLI and MCP clients share the manager's admission limits: four
independent reads per target, within manager-wide bounds of 64 requests, 32
independent reads and eight inspections. Retained IPC separately bounds handshakes,
ordered requests, reads and status. Initial media preparation, probes and
recording finalization remain owned ordered operations. Image publication runs
on bounded blocking workers with separate public/action-evidence capacity;
cancelled workers retain their slots until they actually finish, and session
cleanup drains them before publishing completion. Public reads refuse image
worker saturation; admitted action evidence waits within its operation deadline,
releasing partial reservations if cancelled before a worker starts. Session read
handles carry identity and borrowed capture access, never control authority or
copied journals.
Discovery publishes matching generations without holding its lock over platform I/O.
Empty target lanes retire after their queue drains and native cleanup succeeds;
failed cleanup remains owned. Raw session journals live with run evidence, so
closed-session lookup survives lane retirement without retaining runtime scratch
directories. Retiring a lane leaves the device connected in the manager.

`tests/integration/emulator.rs` runs the AVD lifecycle through shipping MCP with
an owned fake adb and emulator: boot refusal and launch arguments, cancellation,
repeated connect, launch failure and manager death. Other fake-adb suites set an
absent `ANDROID_AVD_HOME` so the host's AVDs never appear in their listings.

`android_contract/scheduling` and `target_scheduling` exercise shipping MCP/CLI
with owned adb processes: blocked discovery/startup/cleanup, read saturation,
status/input progress, overload, FIFO, cancellation, process disposal and journal
isolation and target churn with archived lookup after retirement. The media-helper
fixture checks independent prepared reads. Native
host fixtures cover image-worker lifetime and the retained platform adapters;
selected-device acceptance remains distinct. See the
[iOS guide](ios.md).

`targets/capabilities.rs` owns shared capability names and adapter limits;
`qa/capabilities.rs` owns selected-session verification/admission. `ui` normalizes
accessibility roles while retaining native names. Managed inspection and media
reuse the existing controller through bounded, authority-fenced local requests.

Capabilities live directly under `src/`, with role submodules inside each
capability. Public CLI and MCP entry points compose the same QA workflows.

| Owner | Responsibilities |
| --- | --- |
| `catalog` | Tool descriptions and schemas, independent of MCP transport |
| `cli` | Argument parsing, command application and terminal presentation |
| `protocol` | MCP dispatch, bounded stdio and result/image presentation |
| `managed` | Client identity, the manager's device pool and per-device queues, target resolution, device actor connection, release and recovery, action validation, observations and durable history |
| `qa` | Run/session workflows; `scheduling` owns target runtimes and barriers, `reads` owns borrowed contexts, revocation and image-worker bounds; session-scoped journals preserve outcomes |
| `mobile` | Pure mutation rules and platform dispatch in its adapter submodule |
| `android` | adb transport, input, app operations and pure hierarchy translation |
| `playback` | Media argument/capability contracts, persistent Android helper and independent Indigo consumer-button owner |
| `device` | `Runners` registry and readiness publication; `preparation` owns signed runner build/install/start and rollback, `rpc` owns authenticated USB calls |
| `native` | Simulator worker entry points and worker-local FFI handles; no run/session authorization or controller selection |
| `media` | Preview registry, packet rules, viewer authentication, grants and platform workers |
| `device_media` | Physical source registry, startup/streaming, audio probes with native AAC-ELD decode, observation subscriptions and cancellation-safe cleanup |
| `simulator` | Incarnation validation, controller-owned input transport, single-flight framebuffer sources, bounded native IPC, subscriptions and retained teardown |
| `interactive` | Private controller endpoint ownership, framing and retained server workflows |

Shared code has a specific contract: `ui` owns geometry, editable-focus detection
and gesture bounds; `artifacts` owns private evidence publication; `process` owns
bounded child processes; `lease` owns kernel-held target authority. Workflow
decisions stay with their capability rather than moving into a general utilities
module. Module comments describe ownership and visibility limits internal APIs.
Physical observation decoding and JPEG encoding run on an owned bounded worker
in `device_media/observation`; they do not run inside the shared media receiver.
QA lifecycle teardown retains failed close results and releases terminal recording
bookkeeping only after cleanup succeeds. Runner preparation keeps its host child
and files local until readiness; error and cancellation roll them back.

In `runner/ios`, `TestrunRunnerTests.swift` owns session state and routing.
`RunnerTransport.swift`, `RunnerScope.swift`, `RunnerInput.swift` and
`RunnerObservations.swift` separate framing/journals, scope resolution, gestures
and snapshots. `RunnerRequests.swift` owns locked admission and inspection budgets
outside the UI executor. `tests/runner-transport.sh` compiles production transport
against a fake XCTest executor and uses real authenticated TCP requests to verify
heartbeat/busy replies during blocked reads, expiry, caller cancellation, subsequent
read/input recovery, deduplication and explicit owned-runner stop. The native
input fixture separately checks that observation issues do not quarantine input
and cannot clear an existing mutation quarantine. These fixtures do not establish
physical snapshot performance or device-side teardown.
Objective-C adapters retain focused-input and concrete-tap ABI
boundaries. `native/simulator.m` keeps the simulator ABI and framework handles
together; Rust isolates unsafe calls in `native/bridge.rs`.

Simulator input and capture have separate process/lifecycle owners. The controller
uses `simulator/device_set` for canonical selection and set-scoped command routing;
`simulator/lifecycle` retains boot tasks, kernel exclusion and durable cleanup
records across caller cancellation. The controller
retains `simulator/input`; `native/input` validates and dispatches its bounded
private requests. `simulator::incarnation::Incarnation` binds the native-selected device record,
boot/runtime, device/data directory identities and CoreSimulator service PID/birth
time. Native handles also validate their XPC connection. Warm requests validate
that binding without `simctl list`; discovery remains an acquisition operation.
The session acquires its identity witness independently; accessibility and app
state validation do not open capture or input. Each native worker validates its
own binding against that witness before use.
Compact stills render from a current IOSurface directly, and input geometry reads
do not encode an image. Failed operations retain their cause independently from
release/reaping results; only unresolved disposal keeps cleanup failed.
Input and capture each keep typed requests in their own `protocol` submodule
and pipe/process I/O in `transport`. Shared `simulator/ipc` handles only bounded
reply/event framing. `simulator::cleanup::Cleanup` retains the stop/join result
for either worker and does not conflate an operation error with failed disposal.
The Objective-C handles share the `SBBoundSimulator` device/service interface;
input and identity handles are not treated as capture handles for binding reads.

## Explicit acceptance suites

`cargo test --all-targets -- --test-threads=4` runs all host checks with zero
ignored tests and four concurrent cases. These fixtures launch additional owned
processes and native workers; the case limit bounds that host load while keeping
each case's assertions and deadlines intact. `.cargo/config.toml` sets
`RUST_MIN_STACK=16777216` because unoptimized MCP request futures outgrow the
default 2 MiB test-thread stack. Live platform and download cases are retained
in explicit Cargo acceptance features; enabling a feature fails if its required
environment is missing. A host-suite pass does not establish a live acceptance
result. Run platform suites one at a time; they own their selected resources and
use one test thread. Each command below runs from this package with its
prerequisites in the environment, followed by `-- --nocapture --test-threads=1`:

| Command | Required prerequisite |
| --- | --- |
| `cargo test --features acceptance-ios-device device_media::` | `TESTRUN_TEST_DEVICE`: authorized awake/unlocked USB iOS 27+ serial; captures video and a private clip |
| `cargo test --features acceptance-ios-device --test integration managed_build_handover_and_host_task_end_release_native_ownership` | Same selected phone; read-only build handover and host task end, without recording; optional `TESTRUN_TEST_PREVIOUS_BINARY` for a real executable upgrade |
| `cargo test --features acceptance-ios-device --test integration startup_cancel_after_remote_allocation_releases_before_reacquisition` | Same selected phone; cancel a pending MCP open after observed remote allocation, verify release while the server stays alive, then acquire through CLI; no phone input or recording |
| `cargo test --features acceptance-ios-device --test integration device_media::recovery::` | Same selected phone and `TESTRUN_TEST_STEP_DIR`: one top-center tap whose manager is killed mid-input, a disconnect/reconnect race, lock/unlock with lock-state assertions, cable unplug/replug, and a USB pull during one ten-second top-center long press. Person steps use operator-created markers named in the output; the timed pull uses `PULL-USB-NOW` and must be detected within eight seconds of dispatch. No recording or installation |
| `cargo test --features acceptance-ios-runner physical_owned_runner_authentication_and_typed_errors` | `TESTRUN_TEST_DEVICE_SERIAL` and `TESTRUN_TEST_DEVICE_CONFIG`: authorized phone and private session file for its already running owned runner |
| `cargo test --features acceptance-ios-simulator --test integration simulator::` | `TESTRUN_TEST_SIMULATOR`: owned `Testrun-QA-` simulator with the UIKit fixture below; includes reboot and owned-worker interruption |
| `cargo test --features acceptance-ios-simulator --test integration owned_custom_set_native_boot_capture_and_cli_disconnect` | macOS, selected Xcode, an installed iOS runtime and iPhone 16 device type, or the device type named by `TESTRUN_TEST_DEVICE_TYPE`, such as an iPad; creates and deletes its own `Testrun-QA-Lifecycle-` simulator in a temporary custom set, without guest input |
| `cargo test --features acceptance-android --test integration emulator::native` | `TESTRUN_TEST_AVD`: a stopped owned `Testrun-QA-` AVD with a Quick Boot snapshot; `TESTRUN_TEST_COLD_AVD`: one without a saved snapshot. Starts, freezes and stops them read-only, opens a visible emulator window and kills the manager; checks their data images are unchanged |
| `cargo test --features acceptance-android --test integration android` | `TESTRUN_TEST_ANDROID`: booted owned `Testrun-QA-` emulator; includes UI input, Tab and a probed key chord, app inventory, URL outcomes, a recording (through the shipping binary on macOS) and network persistence with explicit online recovery |
| `cargo test --features acceptance-downloads downloads_and_verifies_pinned_developer_support` | Explicit host internet/cache access; downloads and verifies pinned developer assets without contacting a phone |
| `cargo test --locked --features acceptance-browser-driver --lib browser::` | Explicit absolute `TESTRUN_TEST_NODE`, `TESTRUN_TEST_PLAYWRIGHT_CORE` and `TESTRUN_TEST_BROWSERS` paths; runs browser binding fixtures plus owned Node bootstrap/kill/termination and a shell descendant fixture; no browser launch or downloads |
| `cargo test --locked --features acceptance-browser-lifecycle --lib browser::session::native` | Same explicitly selected private paths; macOS 15 x64, pinned Node 24.12.0 / Playwright 1.64.1 / Firefox 1555 / WebKit 2370. Launches both engines headless and headed with owned ephemeral profiles, exercises context/page/handle lifetimes and orderly descendant cleanup; no external URLs, downloads or device actions |
| `cargo test --locked --features acceptance-browser-lifecycle --lib browser::session::preview_native` | Same three selected private paths; owned Firefox/WebKit headed/headless, JPEG sources, private loopback viewer service, document replacement and viewport growth; no external URLs or mobile devices |
| `cargo test --locked --features acceptance-browser-lifecycle --lib browser::session::settings_native` | Same three selected private paths; owned Firefox/WebKit headed/headless and localhost pages, context creation options, cookies/local-storage/IndexedDB snapshots, page-event cursors, offline/geolocation/permission isolation, media overrides and recording; no external URLs or mobile devices |
| `cargo test --locked --features acceptance-browser-lifecycle --lib browser::session::clock_native` | Same three selected private paths; owned Firefox/WebKit headed/headless, virtual timers, context isolation, host observation/deadline isolation and paused preview/recording; no external URLs or mobile devices |
| `cargo test --locked --features acceptance-downloads --lib browser::preparation::tests::private_downloads` | Downloads the four fixed HTTPS archives into a temporary private root, concurrently prepares both engines and verifies complete offline reuse; no browser launch |
| `cargo test --locked --features acceptance-browser-public --test integration browser::` | macOS 15 x64; real CLI and stdio MCP with a test-owned cache and manager, automatic preparation, both headless engines, a browser shared by CLI and MCP, contexts, evaluation, diagnostic privacy, preview/recording alongside input, screenshots/transfers and disconnect; optional `TESTRUN_TEST_INSTALLED_BINARY=/absolute/path/to/testrun` selects an installed command; no external page URLs or devices |

For example,
`TESTRUN_TEST_DEVICE=SELECTED-UDID cargo test --features acceptance-ios-device device_media:: -- --nocapture --test-threads=1`.
Compile every retained acceptance case without acquiring a target using
`cargo test --all-features --no-run`.

## Owned simulator acceptance

The owned simulator lifecycle suite
(`owned_custom_set_native_boot_capture_and_cli_disconnect`) exercises custom-set discovery, explicit boot,
native capability admission and screenshots through MCP, then managed CLI boot,
disconnect after identity-verified controller death and preservation of an
externally booted fixture. It creates only its
own simulator and set; no existing simulator is selected. This gate does not need
the UIKit app used by the input suite. `TESTRUN_XCRUN` selects the command
executable for discovery, lifecycle and simulator application commands; owned
host fixtures replace that process boundary. Native framework workers still use
the selected Xcode installation and the selected device set.
Set `TESTRUN_TEST_BINARY=/absolute/path/to/testrun` in its environment to run
the same lifecycle gate against a local release executable.

`bash tests/simulator-geometry.sh` compiles the production simulator bridge against a
host-only ownership fixture with a real IOSurface. It checks 5,000 geometry calls
across primary/fallback surface getters, invalid peers, missing surfaces and
exceptions. Every call must release its autoreleased temporaries before returning,
while preserving status codes and scalar dimensions. The test needs host access
to IOSurface when run from an agent sandbox.

Create a new simulator whose name starts with `Testrun-QA-`. Boot it with
simctl. Compile `tests/fixtures/ios/Fixture.m` into a temporary app using the
installed iOS simulator SDK, UIKit/Foundation and the host simulator architecture;
copy its Info.plist, ad-hoc sign, install and launch the fixture on that UUID.

```sh
TESTRUN_TEST_SIMULATOR=OWNED-UUID cargo test --features acceptance-ios-simulator --test integration simulator:: -- --nocapture --test-threads=1
```

The current touch fixture expects the iPhone 17 Pro's 402-by-874 logical display.
It publishes synthetic counter/contact telemetry under its own Documents directory
and a seven-bit/parity visual counter marker. Retained-path tests decode actual
inline JPEGs and compare this independent state, challenge freshness through the
fixture command file, kill only a verified child input worker, and reboot only
the selected owned simulator. These are correctness checks, not latency labs.
Set `TESTRUN_TEST_BINARY` to an absolute installed binary path to test a release
installation instead of Cargo's development executable. Set `TESTRUN_TEST_CODEC=h264`
for the capture test to verify decoder configuration/keyframe delivery; its default
is JPEG.
Tests verify actual down/up phases and field values, secure omission, cancellation
journaling, PNG capture, AX coverage, authenticated preview, MP4 decoding and
broker cleanup. Shutdown/delete only the simulator created for the test.

Android acceptance uses a separately created `Testrun-QA-` emulator selected by
serial; never use a connected phone by default. Physical runner syntax and project
lint are separate from xcodebuild and real-device validation.

On macOS, `bash packages/testrun/tests/concrete-tap.sh` compiles the production
concrete-tap adapter with a host-only XCTest boundary fixture. It verifies callback
failures/timeouts, ABI and admission refusal, exceptions, late/duplicate callbacks
and issue quarantine without contacting hardware. Real foreground/occlusion and
menu-effect acceptance still requires an explicitly selected physical device.

Evidence belongs in private temporary run directories. Preserve artifacts when
a test fails; remove owned fixture artifacts when their investigation is complete.
See [diagnostics verification](diagnostic-verification.md).

## Selected physical video acceptance

Connect and unlock an explicitly authorized iOS 27+ USB fixture with compatible
developer support already available. These tests do not install a runner or
mutate phone UI. The MCP case captures a bounded private clip and checks viewer
authentication, HEVC keyframe/configuration delivery, full-track MP4 decoding,
preview/recording independence, device-session expiration and EOF cleanup.

```sh
TESTRUN_TEST_DEVICE=SELECTED-UDID cargo test --features acceptance-ios-device device_media:: -- --nocapture --test-threads=1
```

This suite includes an MCP disconnect whose native video release succeeds but
whose run summary cannot be published because the fixture placed an owned
directory at `summary.json`. After removing that obstruction, a CLI disconnect
must finish the same cleanup even after the fixture removes the control socket,
retain `EVIDENCE_IO` and skip the completed native
session stages. It sends no phone input. This filesystem failure does not certify
USB-loss recovery or a transient physical audio-release failure.
Before disconnect, it sends a fenced close with an unexpected field to the
selected controller and verifies a validation reply with status still ready.

The native lifecycle suite also verifies build handover, observed readiness,
acknowledged host task end, idempotent CLI task end, and `connect` and
`disconnect` from a client with a different host label. It sends no phone input and makes no recording. By default,
the fixture marks the owned selection as belonging to a different build. To
exercise a real executable upgrade, set `TESTRUN_TEST_PREVIOUS_BINARY` to an
absolute path to a preserved compatible Testrun executable and
`TESTRUN_TEST_BINARY` to the current build. The previous executable starts the
first device actor; the current CLI/MCP must finish its cleanup before replacement.
Do not point either variable at an unverified download or a process you did not start.

Run that focused lifecycle gate with explicit settings:

```sh
TESTRUN_TEST_DEVICE=SELECTED-UDID TESTRUN_TEST_BINARY=/absolute/current/testrun TESTRUN_TEST_PREVIOUS_BINARY=/absolute/previous/testrun cargo test --features acceptance-ios-device --test integration managed_build_handover_and_host_task_end_release_native_ownership -- --nocapture --test-threads=1
```

The `device_media::` suite continues to include the lifecycle case alongside the full capture
and recording acceptance.

The Rust adapter links pinned `idevice` for USB/tunnel/RemoteXPC framing and owns
media negotiation, RTP access-unit validation, receiver feedback and lifecycle.
There is no separate transport runtime or port-forwarding process. Native bindings
in `native/device_media.m` own CoreMedia descriptions and compressed HEVC muxing.
The native observation fixture submits a 129-frame HEVC burst and verifies the
final decoded timestamp and calibrated crop. Source lifecycle tests cancel a close
while remote release is pending and verify that shutdown still awaits that task
and retains its lease. Connection startup errors preserve native diagnostic details.
Temporary phone captures remain private run evidence; do not commit them.

For read-only managed lifecycle acceptance on an explicitly selected awake/unlocked
phone, the following test runs ten connect/observe/disconnect cycles, checking 100
strictly advancing decoded timestamps. It sends no phone input. Point
`TESTRUN_TEST_BINARY` at the installed command to verify that installation:

```sh
TESTRUN_TEST_DEVICE=SELECTED-UDID TESTRUN_TEST_BINARY="$HOME/.local/bin/testrun" ~/.cargo/bin/cargo test --manifest-path packages/testrun/Cargo.toml --features acceptance-ios-device --test integration managed_readonly_repeated_connect_observe_disconnect -- --nocapture --test-threads=1
```

## Individual Rust verification

Run Rust verification categories individually and select only those needed for
the task. A request for tests does not also request formatting, lint,
doctests, API docs, dependency auditing or coverage. Clippy also type-checks
every target. Name the selected commands before running them; there is no
aggregate Rust check command. `cargo doc` generates browsable HTML API
reference in `target/doc/`; `cargo test --doc` executes Rust documentation
examples. Run either as its own explicit category. Rustdoc can recheck
dependencies even with `--no-deps`, which only suppresses their generated
reference pages. Keep Cargo caches between runs.

| Category | Command from this package |
| --- | --- |
| Apply formatting | `cargo fmt --all` |
| Verify formatting | `cargo fmt --all --check` |
| Strict Clippy and type checking | `cargo clippy --workspace --all-targets -- -D warnings` |
| Tests | `cargo test --all-targets -- --test-threads=4` |
| Documentation examples | `cargo test --doc` |
| API reference HTML | `cargo doc --no-deps --document-private-items` |
| Dependency advisories | `cargo deny check advisories` (requires cargo-deny; no license/source policy is configured) |
| Free disk space | `cargo sweep --time 3` (from cargo-sweep), while no build or test uses `target/` |
| Resolve changed local dependencies | `cargo update --offline --package canonic-testrun` |

`cargo fmt --package canonic-testrun` selects Testrun alone; `--all` also visits
local path dependencies. `rustfmt --edition 2024 <files>` formats only the named
files and their child modules.
