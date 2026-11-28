# Investigate iOS and iPadOS failures

The `ios` diagnostic source has separate simulator and physical-device routes.
Both use retained [investigations](debugging.md), private raw evidence and
structured queries. Select an exact target and installed bundle identifier.
No collection operation boots a simulator, launches an app, unlocks/pairs a phone,
enables developer support, installs a helper or changes log privacy.

| Route | What it observes | Prerequisites and limits |
| --- | --- | --- |
| Simulator | Selected CoreSimulator log store; installed app plist/signature; available counters for that simulator app | macOS/Xcode; exact booted UUID/device set |
| Physical USB | Selected phone's log service, installed-app/process observations and supplied exported reports | CoreDevice, preinstalled idevicesyslog, pairing/trust and service access |
| Physical network | Same explicit UDID and observed network transport; no USB fallback | Existing network pairing, CoreDevice and log-service availability |
| Offline import | IPS, crash/hang text, Jetsam, MetricKit and bounded sysdiagnose text | Explicit regular files; no device connection; symbol tools optional |

Simulator/native inventory needs macOS and Xcode's `xcrun`. Physical streaming
also needs an already installed compatible `idevicesyslog`, device pairing/trust
and service access. Xcode/developer-support or lock restrictions can make
individual reads unavailable. Testrun reports those gaps without preparing the
device. No OS/Xcode/libimobiledevice range is certified by the owned fixture tests.
Host architecture/version and available tool versions are retained as observations.

## Collect a selected target

```sh
testrun diagnostics capabilities
testrun diagnostics open --submission-id acme-ios --question "Why did Acme disappear?" --binding '{"targetId":"device:SELECTED-UDID","appId":"com.example.acme","actionIds":["action:EXISTING_REFERENCE"]}'
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id phone-sample --source '{"kind":"ios","targetId":"device:SELECTED-UDID","appId":"com.example.acme","transport":"usb","logSeconds":3}'
testrun diagnostics status diag-RETURNED_REFERENCE
```

The investigation target/app must match the source. For a phone already available
over the selected paired network transport, set `transport: "network"`. Collection
refuses a transport mismatch; it never silently switches to USB or discovers a
different phone. Physical control capability and log-service availability are
independent. Logs come from that device service, never the Mac's Unified Logging.

For a booted simulator:

```sh
testrun diagnostics open --submission-id acme-simulator --question "Inspect simulator failure" --binding '{"targetId":"simulator:SELECTED-UUID","appId":"com.example.acme"}'
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id simulator-sample --source '{"kind":"ios","targetId":"simulator:SELECTED-UUID","appId":"com.example.acme","logSeconds":10}'
```

The default simulator device set is selected explicitly by this source's default.
For a custom set, include its absolute `deviceSet` directory; another connected
actor's set is not inherited. Testrun verifies the exact booted UUID and reads
that simulator's app container. It does not scan the host's global crash folders.

Options are `collectLogs` and `systemContext` (both default true), `logSeconds`
(default 3; simulator lookback 1–30 seconds, physical live sample 1–4 seconds),
optional simulator-only `deviceSet`, and optional `reportDirectory` below.
Every command has at most four seconds or the remaining collector duration.
Shared duration/byte/chunk limits apply; default retained capacity is 1 MiB.
This is one bounded sequence, not a persistent subscription or complete history.

App logs retain currently observed PID/path association. Old log-store rows and
service backlog do not inherit today's process generation. Simulator process
births, when observable, come from the host kernel for the selected simulator
app. Physical process births and simulator boot generation remain unknown.
System context requires a known OS process-name candidate and exact bundle token;
the process name alone does not certify a privileged writer or prove causation.
Neighbor records are excluded. Privacy placeholders and unknown source loss stay
visible; no privacy bypass or log-buffer reset is attempted.

## Inspect failures and available installed state

After the collector settles, use its returned ID:

```sh
testrun diagnostics analyze diag-RETURNED_REFERENCE collector-RETURNED_REFERENCE --submission-id parse-ios
testrun diagnostics analysis-status diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"failure"}' --limit 100
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"process"}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"event"}' --limit 100
```

Queries return witnesses/digests and recorded observations. Watchdog, memory
pressure/Jetsam, background-execution, audio interruption/route and thermal
messages are candidates or context, with `causeEstablished: false`. A termination
namespace/reason stays separate from the exception and backtrace. Nearby events
do not establish the trigger. Missing records never prove absence or healthy state.

Simulator installed build/version, executable and background modes come from the
installed app's plist; selected entitlement fields come from that executable's
signature when readable. Physical installed version/build/path come from device
inventory. Physical entitlements, background modes and resource-profiler state
are unavailable through this route. A local project plist is not substituted.
Simulator RSS/CPU estimates belong to its birth-checked host process and retain
that scope. They are never labeled physical-phone measurements, heap ownership
or an interval CPU profile.

For physical-device CPU measurements during a test, use the
[Activity Monitor native trace](diagnostic-native-traces.md#app-and-render-server-cpu-on-physical-ios)
with the currently observed app PID. It records the app and `backboardd` on the
same trace timeline.

## Reports, MetricKit and deep system text

To inventory already exported reports, add an explicit absolute `reportDirectory`
to the source. It inspects at most 256 entries and selects up to four regular
`.ips`, `.crash` or `.hang` files, at most 8 MiB each. No recursion, symlink
following, global-host search, device crash-report mover or deletion occurs.
Directory/file limits and shared capture limits can leave evidence incomplete.
The directory is caller-selected private scope; it can contain reports for other
apps/devices. A report's own identity remains authoritative for interpretation,
and missing device attribution remains unknown.

For offline diagnosis, import a report directly:

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id report-copy --source '{"kind":"import","path":"/absolute/path/Acme.ips","format":"apple-ips"}' --limits '{"maxBytes":8388608}'
```

Selected formats are `apple-ips` (one JSON object or Apple's header/body JSON),
`apple-crash-text`, `apple-jetsam`, `apple-metrickit` and
`apple-sysdiagnose-text`. IPS/text frames preserve image identity, load address
and raw locations when available. Jetsam process/reason/page data stays system
scope: a matching name/PID alone does not identify the selected bundle. MetricKit
keeps `timeStampBegin`/`timeStampEnd`, producer units/histograms and diagnostic
payloads; delayed aggregates are never converted into live samples.

Physical report transfer is not registered: export through an available approved
Apple workflow and select those files. Sysdiagnose is a separate, explicitly
selected broad text import, bounded by the common import/parser limits. Generation,
ZIP/tar/logarchive expansion and generic secret-free export are unavailable.
No source automatically requests sysdiagnose or uploads evidence. Use existing
audio/media tools separately; this source does not start an audio probe.

## Explicit dSYM/Mach-O symbolication

First import the exact Mach-O or the file inside
`Acme.app.dSYM/Contents/Resources/DWARF/Acme`, as a regular file:

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id dwarf-copy --source '{"kind":"import","path":"/absolute/path/Acme.app.dSYM/Contents/Resources/DWARF/Acme","format":"mach-o"}' --limits '{"maxBytes":8388608}'
testrun diagnostics status diag-RETURNED_REFERENCE
```

Keep its completed collector ID and SHA-256. Directory bundles, partial symbol
copies and files larger than 8 MiB are not accepted for this operation. Then
analyze the report collector with a new submission ID:

```sh
testrun diagnostics analyze diag-RETURNED_REFERENCE collector-REPORT_REFERENCE --submission-id symbolize-ios --metadata '{"symbolicate":true,"symbols":[{"kind":"dsym","evidenceId":"collector-SYMBOL_REFERENCE","sha256":"RETURNED_64_HEX_SHA256","buildId":"IMAGE-UUID-FROM-REPORT","architecture":"arm64"}]}'
```

This explicit option uses retained bytes, a private temporary copy, `dwarfdump
--uuid` and bounded `atos`. Both artifact-internal UUID and exact architecture
must match; frame addresses also require known image load/range information.
No current-checkout lookup or download supplies missing symbols. At most eight
symbol artifacts and 256 frames are attempted, with a separate ten-second budget
and at most three seconds per symbol tool. Unsupported/missing tools, wrong UUID,
architecture, incomplete image identity and address-only output remain unresolved.

Query `kind: "stack"`. A verified result reports
`symbols.state: "resolved_uuid_arch_verified"`, artifact digest/reference and
symbol-tool provenance, while preserving raw frames and the original report.
Producer-provided names and reference matches alone remain `resolved: false`.
Closing/removal/cancellation preserve original reports and symbol files; only
owned temporary copies are removed. See [privacy and retention](diagnostic-access.md).

Source/analysis stop acknowledges cancellation; poll until settled before claiming
cleanup. Normal cancellation/drain reaps owned clients and preserves captured
data. Host/manager hard-loss and native transport/version acceptance remain
unverified. Fixture checks do not certify release devices, actual Xcode symbol
results, complete system reporting or target overhead.
