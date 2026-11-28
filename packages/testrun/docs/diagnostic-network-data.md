# Network requests and data inspection

Start with `diagnostics capabilities` and its `networkData` catalog. Select the
actual source: browser requests, an authenticated HTTP client, RN inspector,
Dart service, proxy HAR, packet capture or app/service data export. Each observes
a different part of the operation. Keep request epochs, account/project scope,
capture coverage, sampling and storage consistency with the evidence.

## Sources and access

| Source | Available route and interpretation |
| --- | --- |
| Browser HTTP/WebSocket | Existing diagnostics, request/response/body, WebSocket and HAR operations; retain a settled `browser_operation`. Native browser filters, cursors, context/document identity and loss remain authoritative. |
| RN/Expo | Existing `react_native` network opt-in. Shared analysis projects recorded CDP request/response/finish/failure/WebSocket metadata with a scope derived from target/page/runtime/context epoch. Native traffic can be absent. |
| Flutter/Dart | Existing Dart HTTP observations and timeline through negotiated extensions; logging must already be enabled. Raw HTTP observations retain their own source semantics; the new view does not guess phase timings from them. |
| Proxy | Existing explicitly selected browser launch/context/HTTP-client proxy and private HAR/body evidence. Use a dedicated owned context/client. Closing it releases its route; no system/mobile proxy setting is changed. |
| Packet headers | Import classic `pcap` for bounded Ethernet/raw IPv4/IPv6 TCP/UDP headers. Original payload stays private raw evidence; no live interface acquisition, TCP reassembly, pcapng, TLS/QUIC decryption or app attribution. |
| HTTP/gRPC/stream/service exports | Import `network-evidence-v1` with explicit protocol, phase units, retry/cache/cancellation, message metadata and scoped request/trace/span IDs. No protobuf/body decoder or universal HTTP/3 visibility is implied. |
| Local app state | Existing browser storage/IndexedDB/cache exports, or app-owned `data-snapshot-v1` for preferences/files/SQLite/Room/Core Data/cache values. Testrun does not open a live database, bypass an app sandbox or decrypt protected storage. |
| Remote SQL/search/cache | A configured access owner executes a named read-only indexed preset and enforces account, resource, row/time/scan/cost limits before dispatch. Bring its scalar result through existing authenticated HTTP access or explicit export. Testrun validates the exported plan and queries the retained snapshot; it introduces no SQL shell, connector, credential store or remote query executor. |

See [browser](browser.md), [RN/Expo](diagnostic-react-native.md),
[Flutter/Dart](diagnostic-dart.md), [investigations](debugging.md) and
[access/export](diagnostic-access.md) for the underlying controls.

Proxy capture needs an explicitly selected route and an already configured proxy.
Use `recordHar`/`har_capture_start` on the owned browser context with a selected
URL filter and private output; content defaults to omission. Close/finalize the
context's capture and retain/import the returned HAR. Certificate trust, TLS
interception and pinning remain prerequisites outside this workflow. Do not
silently disable verification, install a CA or change mobile/system routing.
Proxy support does not establish that every native request traversed that proxy.

## Shared queries

Import and analyze a selected file through the existing tools:

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id network --source '{"kind":"import","path":"/absolute/private/acme.har","format":"har"}'
testrun diagnostics analyze diag-RETURNED_REFERENCE collector-RETURNED_REFERENCE --submission-id network-analysis
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"network_data","networkData":{"operation":"phases"}}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"network_data","networkData":{"operation":"correlate","traceId":"11111111111111111111111111111111"}}'
```

MCP `diagnostics_query` takes those filter fields as tool arguments. `view` is
`network_data`; strict `networkData.operation` is requests/phases/messages/spans/
logs/packets/correlate/data/schema/query_plan. Shared cursor/limit paginate results
(1–200, 256 KiB page ceiling). The view rejects ordinary kind/time/native/memory/
performance filters; ordinary rows/timeline remain available separately. Every
page has collector/digest/capture/source-loss provenance. Original imports can be
deleted after collection without changing retained query replay.

Network operations accept optional exact `id`, `traceId`, or `requestId` with
`identityScope`. A request ID is local to its producer scope, not globally unique.
`correlate` requires either traceId or scoped requestId and returns association
candidates from requests, service spans and logs in that analysis. Query the same
explicit identifiers on other retained analyses to follow separate sources;
there is no fuzzy URL/time/PID join or automatic cross-source latency subtraction.
Missing server spans can mean missing propagation, sampling, loss or coverage.
Equal IDs do not authenticate a service or establish a causal failure.

HAR phases retain milliseconds: missing/null/−1 is unavailable; zero is recorded
zero. TLS (`ssl`) is included in `connect`; phases are never summed into a second
total. HAR wall time remains a string. A single valid lowercase version-00 W3C
traceparent supplies trace/span IDs and the sampling bit; duplicate/invalid/other
version headers leave context unavailable. Cache/retry/cancellation is not guessed
from HAR fields, and a port number never supplies an application protocol.

## Network exchange, version 1

`network-evidence-v1` is a strict JSON object with schemaVersion 1,
`format:"network-evidence-v1"`, bounded `source`, `coverage` and `records`.
The four record types are:

| Type | Required fields; optional fields |
| --- | --- |
| `request` | `id`, `identity`, `protocol`, `method`, `address`, `phases`; optional `status`, `grpcStatus`, `cancelled`, `cache`, `retryOf`. Protocol: http1/http2/http3/websocket/grpc/stream/other. Phases map dns/connect/tls/send/wait/receive/total to nonnegative microseconds or null. Missing entries stay unavailable. |
| `message` | `id`, parent request record ID in `request`, direction sent/received, `sequence`, `bytes`, `encoding`. Sequence is unique per request/direction. Metadata only; no message body is decoded. |
| `span` | `id`, `identity`, `service`, `operation`; optional `durationMicros`, `sampled`. |
| `log` | `id`, `identity`, bounded `message`. |

Identity requires a `scope` and optionally requestId, traceId, spanId,
parentSpanId. Trace IDs are nonzero 32-character lowercase hex and span IDs
nonzero 16-character lowercase hex; span IDs require traceId. All record IDs
are unique in the exchange. Retry references must name another request and be
acyclic. gRPC status is 0–16 and only accepted for protocol grpc. HTTP status is
at most 599. Numeric measurements/byte counts are finite nonnegative values up
to 2^53−1. Phase overlap, missing intermediate lifecycle events and partial
stream observations remain explicit; no total or delivery success is synthesized.

## Scalar data snapshots

For example, an app-owned read-only export can contain:

```json
{
  "schemaVersion": 1, "format": "data-snapshot-v1", "backend": "room",
  "dataset": "orders", "scope": "Acme fixture user", "account": "local",
  "project": "Acme", "role": "cache", "snapshotId": "snapshot1",
  "consistency": "best_effort", "sourceOwner": "app-owned export",
  "coverage": "selected order keys",
  "collections": [{"name": "orders", "indexes": ["state"], "records": [
    {"key": "a", "fields": {"state": null}},
    {"key": "b", "fields": {}}
  ]}]
}
```

Backend: preferences/files/sqlite/room/core_data/indexeddb/cache/remote_sql/
remote_search/remote_cache. Role: authoritative/derived/cache/index. Consistency:
atomic/best_effort/unknown. These and account/project/source/coverage are producer
declarations. An atomic snapshot does not prove two snapshots share a transaction
or represent the current backend. Exporters must handle WAL transactions,
encrypted storage, sandbox permissions, namespace/key selection and privacy.
Raw SQLite/Core Data/Room binaries are not decoded by this format.

Collections have unique names, up to four unique declared index fields and
unique string record keys. Fields are scalar null/boolean/finite number/string,
up to 32 fields per record; strings up to 4096 bytes and exact numbers within
±(2^53−1). Nested objects/arrays are refused. Export them as separate explicitly
keyed collections if needed. `$key` is reserved for the owned primary-key index.

```sh
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"network_data","networkData":{"operation":"data","collection":"orders","key":"a"}}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"network_data","networkData":{"operation":"data","collection":"orders","equals":{"field":"state","value":null}}}'
```

Data selection accepts collection and either key or one exact typed equality on
a declared index. The decoder materializes equality postings in bounded chunks;
indexed queries dereference those postings, with no fallback predicate scan.
Present null differs from a missing field. Without a selector, `data` browses
the bounded retained snapshot. `schema` and `query_plan` accept no record filters.
No SQL, expression evaluation, full-text query, arbitrary predicate or write is
accepted by this interface. The current index is still read/integrity-checked
through the existing bounded retained-analysis owner.

## Configured remote read exports

Configure the provider's read-only principal and named indexed preset in its
existing access service. Enforce approved query plan, timeout, maximum rows,
scanned work and cost there **before execution**. A retained JSON declaration
cannot retroactively prevent a query, undo its cost or prove read-only privileges.

Remote snapshots require `plan` with preset, readOnly:true, indexed:true,
indexName, timeoutMs (1–10000), maxRows (1–2000), maxScanned (at most 1000000),
estimatedScanned, optional actualScanned, costUnit (rows/bytes/read_units), maxCost,
estimatedCost and optional actualCost. Estimates and known actual values cannot
exceed budgets; returned record count cannot exceed maxRows. Unknown actual cost
remains unavailable. The format accepts no SQL or credential fields.

Use existing `http_context_create` credentials/headers/authenticated access,
explicit `http_fetch` of the configured read endpoint with timeout, redirect,
retry and body bounds, then `http_evidence_read` with format json. Retain that
settled body-read operation through `browser_operation`, supplying the exact
original request and operation ID. If its JSON value has
`format:"data-snapshot-v1"`, shared analysis indexes it with source pointers into
`/operation/result/value`; collecting/analyzing/querying never replays HTTP.
The body-read result must itself be JSON. Pages must fit its 64 KiB private-result
limit; larger explicit snapshots can be imported within the shared 8 MiB ceiling.
Do not silently merge pages or assume they share one snapshot generation.

`diagnostics compare.dataSnapshotObservations` compares exact keys/scalar maps
only for matching declared target/app and dataset/scope/account/project. It
reports roles, consistency, source pointers and field digests for up to 100 changed
records, with total count/truncation. Missing records differ from present null.
These observations can reveal a source/cache/index discrepancy; they do not prove
which side is correct or stale. No repair, flush, credential read or retry runs.

Limits: 4096 network/packet records; 32 collections, 2000 data records, four
indexes per collection and 128 ordinals per posting chunk. Classic PCAP supports
both byte orders and micro/nanosecond timestamp fractions, Ethernet with up to
two VLAN headers, raw IP and IPv4/IPv6 link types. Fragments, IPv6 extensions,
truncated transport headers and unsupported link payloads remain unavailable;
unknown link types/pcapng and malformed record lengths refuse. PCAP source offsets
refer to expanded bytes and its timestamps are not aligned to application time.
The shared raw/expanded 8 MiB, JSON/model/row/page and isolated worker limits apply.
Live app/proxy/provider/database/version acceptance and overhead remain unverified.
