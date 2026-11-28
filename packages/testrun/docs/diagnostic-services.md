# Desktop processes and service diagnostics

Inspect one selected process or retain a runtime export through the same
investigation workflow used for mobile diagnostics. Testrun does not discover
and attach to every process, launch a service supervisor or enable a profiler
endpoint. Select the process and its existing lifecycle owner first.

## Read a selected local process

Use an actual PID from the service owner or native tools. Replace the illustrative
PID 42 in both the target and source. `hostId` is a caller-selected host label;
confirm which machine runs the Testrun manager. It is not an SSH destination.

```sh
testrun diagnostics open --submission-id api --question 'Inspect Acme worker resources' --binding '{"targetId":"process:Acme:42","appId":"api"}'
testrun diagnostics start INVESTIGATION --submission-id snapshot --source '{"kind":"host_process","targetId":"process:Acme:42","serviceId":"api","hostId":"Acme","processId":42}'
```

`targetId` must be `process:HOST:PID`. The existing binding's `appId` carries the
service ID (`api` here). Optional binding process fields must agree with the
source. Collection reads one borrowed process and records its observed generation;
it does not signal, suspend, debug or enumerate its children. Parent PID, where
available, does not authenticate the parent's birth or establish a complete tree.

| Collector host | Native read | Measurement limits |
| --- | --- | --- |
| macOS | Kernel birth identity, fixed selected-PID `ps` fields, second birth check | RSS bytes and native reported CPU average; not retained heap or an instantaneous CPU rate |
| Linux | Selected `/proc/PID` stat/status and permitted I/O, boot ID and second birth check | Lifetime CPU clock ticks, RSS bytes, thread count and available I/O counters; no assumed tick frequency or network attribution |
| Windows | Fixed noninteractive PowerShell/.NET selected-process reads with start-time checks | Working set, lifetime CPU milliseconds, threads and handles; parent identity unavailable. Windows native acceptance remains unverified |

Inspect the settled collector, analyze it, then query `kind:"process"` or
`kind:"metric"`. The `host-process-v1` rows retain units, definitions, observed
birth and private native fields. Repeat with `expectedGeneration` set to the exact
previously returned generation to refuse a different process before resource reads.
A denied, exited or changed PID produces an explicit failure; no other PID or
stronger credentials are tried. Only owned helper processes are cleaned up.
Existing collector replay reuses retained bytes rather than resampling a PID.

Each native file read is limited to 64 KiB; native helpers have a three-second
deadline and bounded output. Generic investigation limits also apply. Snapshots
are partial sequential observations. Differences in two snapshots do not establish
a rate without a measured window, compatible birth and counter semantics.
The [native trace](diagnostic-native-traces.md) and [Apple](diagnostic-apple.md)
owners retain their separate profile/report, attach, permission and symbol rules.

## Keep Box as the service owner

When Box owns the workspace/service, use its run/status/operation interfaces to
identify and manage it. Testrun can retain an exact operation's existing log tail:

```sh
testrun diagnostics open --submission-id box-api --question 'Inspect Acme service failure' --binding '{"targetId":"service:Acme:api","appId":"api"}'
testrun diagnostics start INVESTIGATION --submission-id log --source '{"kind":"box_logs","targetId":"service:Acme:api","serviceId":"api","project":"/work/acme","workspace":"acme#feature","operationId":"EXACT_NODE_OPERATION","stream":"stderr"}'
```

This invokes only installed trusted Box's bounded `logs --operation ... --json`
read, with the selected project, workspace and stream. There is no `--follow`,
service start, stop, recovery or command-forwarding option. Selectors cannot be
flags. The response's workspace/operation/stream must match; mismatch refuses.
Box remains responsible for runtime configuration, launch generations, log
rotation, liveness and cleanup. Its native host limits still apply.

`box-log-v1` also accepts explicit Box JSON exports. Operation-tail fields are
`workspace`, `operation`, `stream`, `tail_only`, `text`. Service-tail exports use
`generation_id`, `service`, `stream`, `tail_only`, `text`, with optional native
`live`/`observation` metadata. Up to 64 KiB text is indexed in UTF-8-safe 8 KiB
chunks with byte offsets. A false `tail_only` does not establish complete history
across rotation. Offline logs do not prove a process is alive. Logs stay private
and can contain credentials or customer data.

## Select a runtime artifact

Use the runtime's existing authorized profiler/exporter, then import its explicit
format. Testrun does not install these tools, change app instrumentation, execute
pickle/Marshal, enable remote listeners or invoke converters automatically.

| Runtime / question | Accepted artifact | What the reader preserves |
| --- | --- | --- |
| Node / which recorded stacks recur? | `js-cpu-profile` from `--cpu-prof` or the existing profiler owner | Node tree, samples, producer microsecond deltas when present, script/source positions; no inferred event-loop latency or source-map fetch |
| Bun / which exported JS stacks recur? | Compatible `.cpuprofile` selected as `js-cpu-profile` | Same validated artifact shape; this does not identify Bun as V8 |
| Deno / which sampled path dominates? | `js-cpu-profile` | Producer frames and transpiled source positions; runtime/version remain separate declarations |
| JVM / which thread, GC, lock, allocation or I/O event was recorded? | `jfr-json` from explicit `jfr print --json` | Per-event values, thread IDs, raw timestamp/duration strings and sampled stacks; `correlationId:"event-type:jdk.ThreadPark"` selects a recorded event type |
| .NET / where is recorded profile-axis time spent? | Sampled/evented `speedscope` | Original units and exclusive evented intervals; no EventPipe activation, binary nettrace or dump decoding |
| Go / which CPU, allocation, goroutine or blocking sample value is present? | Raw/gzip `pprof` profile.proto | Each stored sample type/unit separately, labels, locations, mappings and producer functions; no complete heap/GC/lock ownership model |
| Python / which sampled thread stack is present? | Py-spy-compatible `speedscope` export | Thread/profile identity, recorded weights and source frames; no unsafe serialized stats loader |
| Ruby / which frame has exclusive CPU/wall/object samples? | `stackprof-json`, JSON serialization of StackProf's public profile hash | Self samples, inclusive totals, edges and missed samples remain distinct; no invented full stacks |
| PHP / which function or call has recorded cost? | `xdebug-cachegrind` | File/function compression, line positions, self/call costs and exact event columns; 10 ns and bytes remain distinct |
| Windows native / which exception address/thread/module was saved? | `windows-minidump` | Selected little-endian system/thread/module/exception metadata and CodeView byte identities; no memory dereference, stack unwind or PDB acquisition |

Binary JFR/EventPipe, ETW, ELF cores, Python pickle/pstats and Ruby Marshal are not
decoded. An extension alone does not determine a format. Unsupported selections
remain raw retained evidence and return a parser refusal. All imports retain the
shared 8 MiB raw/expanded and 16 MiB index limits; select a bounded export rather
than dumping an entire production recording or archive.

## Preserve service and workload identity

For deployment/process associations, import `service-evidence-v1`. It wraps one
concrete artifact, so existing decoders retain their own units and models:

```json
{
  "schemaVersion":1,
  "selection":{
    "targetId":"service:Acme:api","serviceId":"api","hostId":"Acme",
    "os":"linux","transport":"remote_export","pid":42,
    "processGeneration":"boot:42:birth","runtime":"node","runtimeVersion":"EXACT_VERSION",
    "buildId":"build-a","architecture":"x86_64",
    "scope":{"kind":"host"},"owner":{"kind":"borrowed"}
  },
  "format":"js-cpu-profile","encoding":"json",
  "payload":{
    "nodes":[{"id":1,"callFrame":{"functionName":"Acme.work","url":"acme.js","lineNumber":12}}],
    "startTime":0,"endTime":100,"samples":[1],"timeDeltas":[100]
  }
}
```

Save this producer export as a regular file and import/analyze it:

```sh
testrun diagnostics start INVESTIGATION --submission-id profile --source '{"kind":"import","path":"/private/tmp/acme-service.json","format":"service-evidence-v1"}'
testrun diagnostics analyze INVESTIGATION COLLECTOR --submission-id parse
testrun diagnostics query INVESTIGATION ANALYSIS --filters '{"kind":"stack"}'
testrun diagnostics query INVESTIGATION ANALYSIS --filters '{"view":"flamegraph"}'
```

Wait for collector/parser settlement as described in [investigations](debugging.md).
Stack queries preserve weights and witnesses. Ruby/PHP frame/edge costs remain
metrics, and JFR unsampled events remain events; they are not synthesized into
stack samples. Use multiple collectors in the investigation for native and runtime
evidence. Independent clocks do not become aligned just because they share a PID.

The envelope accepts `encoding:"json"`, `"utf8"` or `"base64"` as appropriate for
its artifact. Binary pprof/minidump use base64; Xdebug text uses utf8. Recursive
service/framework envelopes, commands and unknown fields refuse. Contradictory
decoded PID/host/build/architecture/generation or Box operation identity refuses.
Each indexed row keeps `data.serviceEvidence`, payload digest and source witness.
Imported identity and ownership remain producer claims, not authenticated receipts.

`runtime` is node/bun/deno/jvm/dotnet/go/python/ruby/php/native; `os` is
macos/windows/linux. Scope and transport must agree:

| Scope | Required identity | Transport |
| --- | --- | --- |
| `{"kind":"host"}` | Host ID and process birth | `local_export`, `remote_export` or `box_export` |
| `{"kind":"container",...}` | `containerId`, `namespaceId`, `imageId` | `container_export` |
| `{"kind":"kubernetes",...}` | `cluster`, `namespace`, `podUid`, `containerId`, `restartCount`, `imageId` | `kubernetes_export` |

PID is interpreted inside the selected scope. Host process counters cannot be
relabeled as container or pod metrics. The same PID after a container restart is
a new selection. Remote/container/Kubernetes fields do not authorize SSH, exec,
deployment, credential discovery or privileged tracing; transfer is external.

Owner is `{"kind":"borrowed"}`, `{"kind":"external","invocationRef":"..."}`
or `{"kind":"box","projectId":"...","workspace":"...","operationId":"...","generationId":"..."}`.
Box ownership requires `box_export`; keep Box's exact node generation/operation,
not only a service name. Testrun never starts or stops an imported owner.

Limits include 10,000 JS nodes/samples, depth 256, 10,000 selected JFR events,
4,096 pprof samples with up to 16 value types, 256 resolved frames per sample,
10,000 StackProf frames, eight Cachegrind event columns, 64 minidump streams and
1,024 threads/modules, plus shared row/index/query budgets. Event time units,
sample losses, unresolved symbols and incomplete coverage remain explicit. These
fixture-tested readers do not certify live profiler/runtime/host/version ranges,
production overhead, complete process trees or app health.

If process identity changes, select the intended new birth explicitly. If an
export refuses, choose a supported bounded export with the exact runtime/build
and matching symbols; do not change credentials, import labels or workload scope
to force a pass. Use [evidence assertions](diagnostic-testing.md) for a focused
failure/correction check against retained observations.
