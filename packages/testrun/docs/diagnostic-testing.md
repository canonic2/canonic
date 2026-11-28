# Test results and evidence assertions

Use existing runners to execute tests, then retain their reports and evaluate
Testrun evidence. Report ingestion and assertion queries never run commands,
send app input, open attachment references or acquire device control.

## Run a declared test command

Keep the command in the project's existing check declaration. For example,
inside an existing native profile:

```yaml
development:
  app_id: com.example.acme
  checks:
    ui:
      command: [make, test-report]
      timeout_ms: 120000
```

`make test-report` is a project-owned target using its selected test runner and
export format. Testrun does not create that target or install its dependencies.

```sh
testrun project start ios-dev --task check --check ui --submission-id ui-1
```

The existing project operation owns exact argv, environment, deadline, private
logs, exit status, cancellation and recovery. Keep its operation ID as the report's
`invocationRef`. See [project checks](projects.md). A zero exit status does not
prove every test ran; retain the actual report and inspect skipped/unavailable
cases. Query failure does not rerun the command.

This execution route uses existing native-profile checks. For other project
kinds whose check adapter is unavailable, keep the declared invocation with its
existing external execution owner and import its report. Do not create a fake
native profile to run a web/package check.

| Runner | Retained result route |
| --- | --- |
| XCTest / Swift Testing | Explicit normalized export; JUnit when the configured producer emits the supported dialect. Native `.xcresult` bundles are references only |
| Android instrumentation / Espresso / UI Automator | Existing declared command and selected JUnit/normalized export |
| Flutter / Dart | Existing declared test command and selected JUnit/normalized export; machine reporter streams are not decoded directly |
| Web / package-native / external QA | Existing declared invocation and selected JUnit/normalized export |

Before a runner drives a device, release competing Testrun input ownership and
let the selected runner own control. Keep passive collectors only where their
contracts permit coexistence. Do not send Testrun input concurrently. Automatic
controller handoff and live runner/device compatibility are unregistered here;
an imported `control` label is not a verified ownership receipt.

## Import test results

Import `junit-xml` for bounded UTF-8 `<testsuite>`/`<testsuites>` reports. The
selected dialect supports testcase `name`, `classname`, time in seconds, and
`failure`, `error`, `skipped` result tags. Failures map to `assertion_failed`,
unclassified errors stay failure rows with outcome `unavailable`: a JUnit error
alone cannot distinguish automation, fixture or application exceptions. Empty
failure/error/skipped tags retain their outcome. Unknown
result extensions, DTDs/entities, duplicate attributes, malformed nesting and
ambiguous multiple result tags refuse. Standard XML character references are
supported. System output/properties remain only in private raw evidence.

JUnit alone does not authenticate build/device/attempt identity, capture coverage
or attachment ownership. Cases get document-local ordinal IDs; pair them with the
analysis/digest. This decoder records incomplete identity/coverage even when all
parsed cases pass. It is not a general XML or xcresult archive reader.

Use `test-results-v1` for explicit runner/case/attempt associations and private
attachment/capture declarations:

```json
{
  "schemaVersion":1,"runId":"run-1","runner":"xctest","invocationRef":"build-CHECK-ID",
  "control":"testrun_released","buildId":"build-a","targetId":"simulator:SELECTED",
  "coverage":"complete",
  "cases":[{
    "id":"checkout","attempt":1,"name":"checkout renders","outcome":"assertion_failed",
    "message":"missing button","durationMs":12,"actionIds":["tap-1"],
    "attachments":[{"kind":"screenshot","name":"failure","reference":"capture-1"}],
    "capture":{"strategy":"prebuffered","beforeMs":1000,"afterMs":500,"limitBytes":1048576,"lost":false},
    "checks":[{"id":"button","domain":"ui","outcome":"assertion_failed","message":"not visible"}]
  }]
}
```

Runners: `xctest`, `swift_testing`, `android_instrumentation`, `espresso`,
`ui_automator`, `flutter`, `dart`, `web`, `package_native`, `external`.
Control: `none`, `testrun_released`, `external_owned`, `competing`, `unknown`.
Coverage: `complete`, `partial`, `unknown`.

Case outcomes: `passed`, `assertion_failed`, `automation_failed`, `fixture_failed`,
`unavailable`, `skipped`, `cancelled`. Check outcomes: `passed`, `assertion_failed`,
`unavailable`. A passed case with a failed/unavailable nested check is refused.
Duplicate case ID/attempt pairs refuse; attempts are numbered 1–1,024.
Run/case/action/attempt IDs are row correlations. `invocationRef` and `durationMs`
may be omitted; arrays and capture declarations are explicit.

Attachments accept `log`, `crash`, `profile`, `screenshot`, `video`, `audio`,
`trace`, `junit`, `xcresult`, `other`, with a label/reference and optional exact
SHA-256. References remain producer claims; the parser never opens them. Match
them against the owning Testrun operation/artifact before reading an attachment.

```sh
testrun diagnostics start INVESTIGATION --submission-id report --source '{"kind":"import","path":"/private/tmp/tests.xml","format":"junit-xml"}'
testrun diagnostics analyze INVESTIGATION COLLECTOR --submission-id parse-report
testrun diagnostics query INVESTIGATION ANALYSIS --filters '{"kind":"failure"}'
```

Wait for collector/parser settlement as described in [investigations](debugging.md).
Shared raw/expanded/index bounds apply. There are at most 1,024 cases, 32 checks,
16 action IDs and 16 attachment references per case, 8 KiB messages and 64 KiB per
derived row. Oversized records refuse rather than silently dropping failures.

## Assert retained evidence

Use `diagnostics query --filters` or `diagnostics_query` with `view:"assertions"`:

```json
{
  "view":"assertions",
  "assertions":{"checks":[{
    "id":"checkout-pass","domain":"ui","name":"checkout renders",
    "predicate":{"type":"equals","pointer":"/test/outcome","expected":"passed"}
  }]}
}
```

Checks select exact row `kind`, `name`, and/or `correlationId`. Domain labels are
`ui`, `api`, `accessibility`, `visual`, `media`, `performance`; they do not grant
source authority or prove accessibility conformance. Compose existing UI/API/
image/media evidence owners, inspect the retained row's `data`, then select an
appropriate fixed predicate:

| Predicate | Meaning |
| --- | --- |
| `count` with required `min`, optional `max` | Bounds on selected observations. Missing observations/upper bounds need complete coverage; an observed excess is a contradiction |
| `equals` with `pointer`, scalar `expected` | Every selected row has the exact scalar at the JSON pointer relative to `data`. Missing and explicit null differ |
| `metric` with `unit`, `definition`, `aggregation`, optional `min`/`max` | Exact row measurement semantics; aggregation `all`, `mean`, `max`; at least one bound required |

Metric checks require complete observations and matching units/definitions;
they never convert milliseconds/bytes, infer measurement units or treat missing
values as zero. Select agreed scenario bounds and retain their baseline evidence.
For matched-run sample counts, variation, warm state/instrumentation and
measurement uncertainty, use [controlled comparisons](diagnostic-reproduction.md).
An observed maximum or passing bound does not establish a tail guarantee or
statistical significance.

There are at most 32 checks and 200 selected rows per check, with eight ordinal
witnesses. Larger selections are `unavailable`, with a lower-bound count and a
truncation flag. Use narrower selectors. Assertion view accepts no generic row
filters, cursor or limit. Expressions, scripts and arbitrary predicates are not
supported.

Outcomes remain distinct: `passed`, `assertion_failed`, `automation_failed`,
`fixture_failed`, `unavailable`. Known competing control, runner failures and
skipped/unavailable/cancelled evidence cannot become a successful assertion.
The result includes raw/index digests, collector/analysis IDs and row ordinals;
query those rows to investigate and verify a correction against new evidence.
The CLI's success exit means the query ran; inspect `outcome` for the test result.

## Failure evidence and pre-event limits

Start the selected existing collector, trace or recording before the action when
earlier evidence is needed. After observing failure, stop/finalize through that
owner and retain the settled result. Source duration, bytes/events, source-loss
and controller/profiler conflicts remain in force. A later capture cannot
reconstruct missing earlier data.

The report's capture strategy is `none`, `prebuffered` or `post_failure`.
Before/after windows are bounded at 300 seconds and the declared byte budget at
8 MiB. Only `prebuffered` may declare nonzero `beforeMs`; `none` requires zero
windows/budget. Loss remains explicit and prevents complete-coverage assertions.
These are unverified producer bounds. Automatic failure triggers and a new
pre-event ring buffer are unregistered. Live runner/device/control acceptance
and native binary attachment decoding remain separate work.
