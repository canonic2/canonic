# Browser evidence in investigations

Use `testrun diagnostics capabilities` to discover browser investigation routes
alongside mobile and imported evidence. Its `browser.engines` entries reuse the
Chrome, Firefox and WebKit contracts. Host support is separate from dependency
and selected-session readiness; use `browser_doctor` for a readiness check.
Discovery does not prepare dependencies, launch a browser or begin instrumentation.

The `browser_operation` source copies one settled operation from an explicitly
selected connected browser. Capture and advanced analysis keep their existing
browser operations, identities, permissions and cleanup. No request is replayed
by this source, including requests that originally paused, profiled or changed
the page. Stop/close/removal affect the investigation's copy only.

## Retain a browser result

Connect and use the [browser guide](browser.md) to select an owned context/page.
For example, submit a native diagnostic read with explicit private text selection:

```json
{
  "operation": "diagnostics_read",
  "context": {
    "connectionId": "CONNECTION-UUID",
    "id": "CONTEXT-UUID",
    "generation": "CONTEXT-GENERATION-UUID"
  },
  "query": {"after": 0, "limit": 20, "includeText": true}
}
```

Submit it through `browser_send` or `testrun browser send`. Keep the request and
returned operation ID; poll `browser_status` until settled. Start subscriptions,
profiles, traces or debugger work explicitly through their existing requests.
A read does not automatically start a missing subscription or fill older history.
An expired cursor requires the browser owner's documented recovery, not a replay
of the triggering action.

Open an investigation bound to the returned browser target:

```sh
testrun diagnostics open --submission-id acme-browser --question "Why did the page fail?" --binding '{"targetId":"browser:CONNECTION-UUID","actionIds":["action:EXISTING_REFERENCE"]}'
```

Then collect the settled operation, supplying that exact original native request:

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id browser-read --source '{"kind":"browser_operation","targetId":"browser:CONNECTION-UUID","operationId":"OPERATION-UUID","request":{"operation":"diagnostics_read","context":{"connectionId":"CONNECTION-UUID","id":"CONTEXT-UUID","generation":"CONTEXT-GENERATION-UUID"},"query":{"after":0,"limit":20,"includeText":true}}}'
testrun diagnostics status diag-RETURNED_REFERENCE
```

The typed request fingerprint must match the original operation. Omitted defaults
are normalized by the existing browser request codec. Use absolute native artifact
destinations; relative paths normalized before the original submission must be
supplied in their normalized form. For shared `act`, supply the translated native
batch request, rather than the outer action envelope. A different request, expired
operation or missing browser refuses without submitting anything. A running
operation requires polling its original owner before retrying this collection.

The canonical request has a 64 KiB ceiling; the selected envelope has an 8 MiB
ceiling and is copied in 16 KiB chunks under the common duration/byte/chunk limits.
Default capture capacity is 1 MiB. A clipped envelope remains private raw partial
evidence and can fail structured decoding. Select smaller native query pages when
necessary. A successfully copied envelope is still a partial observation of the
browser, with reason `browser_operation_snapshot`.

Reusing the same investigation submission ID and arguments returns the retained
collector even after the browser closes or forgets the original operation. A new
submission needs a still-retained native operation. The copy does not extend the
native owner's retention or pin a model, file, object handle or process.

## Query retained results

```sh
testrun diagnostics analyze diag-RETURNED_REFERENCE collector-RETURNED_REFERENCE --submission-id parse-browser
testrun diagnostics analysis-status diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"failure","correlationId":"action:EXISTING_REFERENCE"}'
```

`browser-operation-v1` indexes operation outcomes and bounded native result
fields/array entries with exact raw witnesses. Diagnostic page-error and
request-failed events become failure observations; other results retain native
meaning without inferred units, causes or synthetic CPU samples. Producer cursor,
dropped/filtering flags, private text, native timing and worker/frame references
remain in their original fields. The parser does not invent a shared clock from
a producer timestamp. A field larger than 32 KiB remains a raw witness with an
explicit structured-row gap; it is not silently truncated into a complete result.

The packet retains the original request's typed references and their current
liveness when copied. Navigation can make an old frame reference stale; collection
never retargets it to the current document. Current selection does not establish
the scope of a past operation. Browser/session/resource generations are not renderer
PID births. Renderer process IDs and their birth mapping remain unknown. Imported
packets are producer assertions, not authenticated browser observations. A project
app binding does not turn all browser results into verified app-isolated evidence.

## Preserve native capabilities and artifact ownership

| Evidence | Existing browser owner and investigation route |
| --- | --- |
| Console, errors, requests, WebSockets, private arguments | Native diagnostics/read/argument operations; retain their settled reads and original cursor/loss/engine restrictions |
| Response bodies and request details | `network_read` and network evidence operations; retain returned fields/references, with private-content selection unchanged |
| HAR and coverage | Existing HAR/profile operations own capture/finalization; retain their receipts or bounded query output |
| Debugger | Existing page/automation debugger operations own pause identity, callbacks and cleanup; copying a status/query does not attach, pause or resume |
| CPU profiles and traces | Existing profile/trace operations own capture and files; retain returned receipts; explicit small raw imports can use a registered matching decoder |
| Heap and performance analysis | `heap_query`, `heap_compare`, `performance_query` use the existing engine models; retain those query results without rebuilding their models |

Chrome trace-event JSON and Playwright trace ZIP are distinct formats.
`performance_query` and heap queries retain their native availability and identity
rules; a generic receipt is not a trace/heap decoder. Native files can exceed the
shared 8 MiB import ceiling. No automatic archive expansion, artifact-path read,
source-map lookup, remote attachment or renderer-process discovery runs here.
Retaining an evidence-list receipt does not copy the listed files. Explicit file
imports make independent copies; otherwise use the existing owner before releasing
its model or removing its artifact.

Raw packets include the selected request/result and can contain secrets. Ordinary
status omits them; reads and queries expose private content deliberately. Native
capture/query permissions still apply before the copy. See
[access and retention](diagnostic-access.md). Owned fixtures verify borrowing,
generation/mismatch refusal, retained querying and preservation; they do not
establish additional live engine, host, navigation or profiler acceptance.
