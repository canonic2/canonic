# Interactive debugging and instrumented builds

Use `diagnostics capabilities` to inspect `interactiveDebugging` before attaching
or changing execution. Chrome JavaScript debugging is available through the
existing owned browser session. Native LLDB, JDWP and DAP transports, interactive
Hermes/Dart attachment and native temporary probe insertion are unavailable.
Firefox/WebKit's automation debugger controls automation calls, not JavaScript.

## Inspect an owned Chrome pause

Send these requests with `browser_send` and your connected browser `targetId`.
`page` must be its exact owned page reference. See [browser debugging](browser.md)
for controller ownership and the shared `act` wrapper.

```json
{"operation":"page_debugger","page":{},"action":{"kind":"enable","exceptions":"uncaught"}}
```

Replace `{}` with the reference from `page_list`. Enable changes exception-stop
behavior. Set an exact source breakpoint or request `pause`, then poll
`{kind:"status",includeFrames:true}`. Status reports a debugger `sessionId`, the
page reference, stopped reason, a fresh `pauseId`, bounded frames and scope
indices. Ordinary status omits frames and variable contents. The selected page
is the JavaScript execution target; this is not native process/thread inspection.

For a returned frame and scope, send:

```json
{"operation":"page_debugger","page":{},"action":{"kind":"variables","pauseId":"RETURNED-UUID","frame":0,"scope":0,"offset":0,"limit":20}}
```

This reads own property descriptors with `Runtime.getProperties`. Getters are
skipped, nested objects are not expanded, and native object IDs stay private.
Values and names are capped at 128 characters; inspect `stringCharacterLimit`.
Up to 100 entries and 6 KiB of variable-array JSON are returned per page. Use
`nextOffset` for pagination; at most the first 10,000 descriptors are accessible.
A resume or another stop during the read discards the result as stale. Variable
contents are private evidence. Object proxies and debugger/runtime behavior can
still affect the target; descriptor access does not evaluate caller expressions.

`evaluate` runs an expression in a paused frame and may execute arbitrary code.
Breakpoint conditions also execute JavaScript. `resume` and `step` require the
current pause identity. Status, variable reads and cleanup remain responsive
while an application operation is paused.

Same-page CPU/coverage profiles and the debugger are exclusive. Stop the current
owner before starting the other; Testrun does not take it over. A pending or
failed cleanup retains ownership until acknowledged. `disable` releases the
owned debugger and its breakpoints; browser cleanup attempts this release.
Navigation/context loss invalidates pauses, and target exit invalidates page
references. Transport loss cannot establish that a resume reached the target:
inspect connection/cleanup status before reconnecting or sending more input.
Testrun does not attach to or resume an unrelated native app.

## Add and remove a temporary logpoint

```json
{"operation":"page_debugger","page":{},"action":{"kind":"logpoint_set","url":"https://example.com/app.js","line":12,"column":0,"marker":"Acme:checkout-entered"}}
```

Line and column are zero based. The marker is a literal, at most 256 bytes.
Testrun generates a conditional breakpoint that calls `console.debug` with a
JSON string containing `testrunMarker`, then returns false. It does not evaluate
an agent-supplied expression. The console method is app code and may have been
replaced; the call changes execution and timing and is not guaranteed to be
side-effect free or to avoid a stop if it throws. Existing console diagnostic
capture can retain the marker text.

Status lists `temporaryLogpoints` separately from ordinary breakpoint IDs.
Remove the returned `breakpointId` with `breakpoint_remove`; uncertain insertion
keeps a pending owner until debugger disable/cleanup. Acknowledged removal changes
only that breakpoint. Disable removes this debugger's remaining probes and
breakpoints, preserving the application's intentional console statements.

## Declare a diagnostic build

Add `diagnostics` to an existing v1 build profile or a v2 profile's `build` block:

```yaml
diagnostics:
  symbols: external
  profileable: true
  sanitizers: [address, undefined]
  coverage: false
  probes: [Acme:checkout-entered]
  timing: instrumented
```

All six fields are required in the effective declaration. `symbols` accepts
`none`, `embedded`, `external`, `unknown`; `timing` accepts `production_like`,
`instrumented`, `unknown`; sanitizers accept `address`, `thread`, `undefined`.
These are profile claims, not verified binary properties. Effective build records
and produced artifacts retain them with `verification:profile_claims_not_binary_verified`.
Absence means unknown. Configure the actual diagnostic flags in the project's
existing Xcode build settings, Gradle tasks/properties, Expo profile, Flutter
mode or custom build command. Testrun adds no flags, probes, signing changes,
dependencies or manifest edits from the declaration. Select the profile through
the normal [build operations](builds.md); install and launch remain explicit.

## Analyze sanitizer and runtime-check reports

Import `sanitizer-report-text` for bounded UTF-8 ASan/TSan/UBSan reports. It
recognizes `ERROR: AddressSanitizer:`, `WARNING: ThreadSanitizer:` and
`SUMMARY: UndefinedBehaviorSanitizer:` markers, preserves subsequent raw blocks
as failure rows, and makes no process attribution or stack symbolication.
Preamble before the first marker remains only in the original raw evidence.
Use `runtime-checks-v1` for normalized StrictMode, race/lock or other runtime
checks and explicit producer stack/identity declarations:

```json
{
  "schemaVersion":1,
  "targetId":"android:SELECTED",
  "appId":"com.example.acme",
  "processGeneration":"boot:pid:start",
  "buildId":"exact-build-reference",
  "timing":"instrumented",
  "environment":["ASan enabled; debug allocator"],
  "coverage":"selected instrumented process",
  "reports":[{
    "id":"fault-1","tool":"address_sanitizer","category":"memory",
    "message":"heap-use-after-free","threadId":"T1",
    "stacks":[{"role":"fault","threadId":"T1","frames":["read /src/main.cc:4"]}],
    "rawReference":"retained-asan-log","fatal":true
  }]
}
```

Unknown fields are refused. Tool names: `address_sanitizer`, `thread_sanitizer`,
`undefined_behavior_sanitizer`, `strict_mode`, `race_detector`, `lock_checker`,
`runtime_check`. Categories: `memory`, `race`, `lock`, `undefined_behavior`,
`policy_violation`, `assertion`, `other`. Stack roles: `fault`, `allocation`,
`free`, `conflicting_access`, `lock_acquired`, `thread_created`, `other`.
`threadId` and `fatal` can be omitted; stack `threadId` is optional too.

```sh
testrun diagnostics start INVESTIGATION --submission-id checks --source '{"kind":"import","path":"/private/tmp/asan.txt","format":"sanitizer-report-text"}'
testrun diagnostics analyze INVESTIGATION COLLECTOR --submission-id parse-checks
testrun diagnostics query INVESTIGATION ANALYSIS --filters '{"kind":"failure"}' --limit 20
```

Wait for collection/analysis settlement as described in [investigations](debugging.md).
Reports have no invented timestamps. Filter normalized records by report ID with
`correlationId`. Raw references and identities are unverified producer claims.
No source file, executable or report reference is opened by the parser.
Timing/environment differences stay attached to each finding. Empty imports or
missing reports cannot establish a clean run; analysis reports incomplete
coverage. This workflow does not enable sanitizers or repair code automatically.

Shared limits are 8 MiB raw/expanded, 64 KiB per row and 16 MiB derived index.
Normalized inputs allow 1,024 reports, eight stacks per report, 64 frames per
stack, 32 environment labels and 16 KiB messages. Native text allows 1,024 blocks,
256 lines per block and 4 KiB per line, additionally subject to the row limit.
Large reports are refused rather than silently truncated.

Protocol and parser fixtures cover these paths. Live debugger/build/native
runtime-check compatibility and overhead remain unverified.
