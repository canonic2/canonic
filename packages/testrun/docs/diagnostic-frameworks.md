# Frameworks, embedded pages and game evidence

Combine a framework's exported runtime observations with native diagnostics in
one investigation. Choose an actual transport or supported export; a framework
name or target category does not establish that a debugger, profiler or input
driver is available. Use `diagnostics capabilities` before acquisition.

## Observe an embedded CDP page

This source reads an already enabled numeric loopback HTTP inspector endpoint.
Observation currently requires a Unix collector host for the existing kernel
lease. Non-Unix observation returns `UNSUPPORTED_HOST`; discovery/import remain
separate. An externally forwarded WebView2 endpoint still requires that host
boundary; this does not add a Windows attachment controller.
For an Android webview, arrange forwarding for the explicitly selected app
separately. Testrun does not scan ports, enable webview debugging, change launch
arguments/registry settings or start/forward an inspector.

Open an investigation bound to the selected native target and app, then discover:

```sh
testrun diagnostics open --submission-id embedded --question 'Investigate Acme bridge failure' --binding '{"targetId":"android:ACME","appId":"com.example.acme"}'
testrun diagnostics start INVESTIGATION --submission-id discover --source '{"kind":"embedded_cdp","endpoint":"http://127.0.0.1:9222","targetId":"android:ACME","appId":"com.example.acme"}'
```

Read the collector to select the exact page ID and URL. Discovery lists at most
64 entries and opens no websocket. Native app/process association is caller
declared; page URL and inspector ID do not authenticate an installed app.
Multiple pages are independent. No first-page or URL-substring selection occurs.

```sh
testrun diagnostics start INVESTIGATION --submission-id observe --source '{"kind":"embedded_cdp","endpoint":"http://127.0.0.1:9222","targetId":"android:ACME","appId":"com.example.acme","mode":"observe","pageId":"RETURNED_ID","expectedUrl":"https://example.com/acme","allowDebuggerAttachment":true,"network":true,"sampleMs":1000}'
```

`allowDebuggerAttachment:true` acknowledges that attaching to a borrowed page
can affect another debugger; the local endpoint lease cannot fence external
clients. Coordinate before attaching. Testrun enables Runtime, Page and Log,
plus Network when requested. Unsupported optional domains are recorded as gaps;
Runtime refusal fails collection. No expressions, breakpoints, binding injection,
profiler commands, page input or navigation commands are sent.

Exceptions, console/log events, inspector-visible network records and navigation
retain raw witnesses. Preinstalled app instrumentation may emit bridge IDs through
these events; observing a JS error alone does not prove its native cause. Pair
it with selected native evidence. Webview navigation, context destruction/clear,
crash, detach or transport loss ends the window; rediscover explicitly. There is
no reconnect, native PID authentication or silent retargeting. A protocol that
silently rebinds without events remains a gap.

The observation window is 100–10,000 ms after setup, at most 256 selected events,
256 KiB selected event data, 32 KiB per event and 1 MiB per packet. Generic
investigation duration/byte limits also apply. Oversized events and buffer loss
remain gaps; this is never complete page history. Cancellation closes only the
owned websocket and lease. The borrowed page, app and enabled endpoint remain
under their existing owner. Endpoint and event payloads stay in private evidence.

After settlement, analyze the collector and query `kind:"failure"`, `marker`
or `event` through the ordinary investigation API. The retained format is
`embedded-cdp-v1`. Its runtime generation is a capture epoch, not a native birth ID.

## Combine exported runtime and native evidence

Import `framework-evidence-v1` with inline components. Each component selects an
existing decoder; no reference is opened, converter executed or binary archive
unpacked. This minimal export combines an app-owned coroutine observation and an
Android snapshot (illustrative producer data, not proof of live collection):

```json
{
  "schemaVersion":1,"framework":"kotlin_multiplatform","frameworkVersion":"EXACT_VERSION",
  "target":{"id":"android:ACME","appId":"com.example.acme","class":"phone","os":"android","transport":"import"},
  "build":{"id":"build-a","architecture":"arm64","mode":"debug","aot":false,"trimmed":false},
  "components":[
    {"id":"shared","role":"runtime","processGeneration":"boot:pid:birth","runtimeGeneration":"runtime-1","buildId":"build-a","architecture":"arm64","format":"runtime-samples-v1",
     "payload":{"schemaVersion":1,"runtime":"kotlin","buildId":"build-a","symbolsBuildId":"build-a","architecture":"arm64","records":[
       {"name":"Acme load","category":"coroutine","identity":"job-1","state":"suspended","frames":["shared.Acme.load(Acme.kt:12)"],"actionId":"load-1"}
     ]}},
    {"id":"native","role":"native","processGeneration":"boot:pid:birth","buildId":"build-a","architecture":"arm64","format":"android-diagnostics-v1",
     "payload":{"schemaVersion":1,"appId":"com.example.acme","kind":"state","data":{"status":"ok","fields":{},"metrics":[]}}}
  ],
  "links":[{"id":"load-1","kind":"bridge","from":{"componentId":"shared","ordinal":0},"to":{"componentId":"native","ordinal":0}}]
}
```

Use `diagnostics start` with an absolute import path and
`format:"framework-evidence-v1"`, then analyze and query its rows. Component
summaries have `kind:"process"`; `correlationId:"component:shared"` selects its
observations. `correlationId:"load-1"` selects the explicit link, whose
`witnessOrdinals` point into the same derived index. Links express producer
claims, not automatic clock alignment or causal proof. Their component-local
ordinals refer to decoded child rows before composition.

Each object row keeps its existing decoder fields plus `data.frameworkEvidence`
provenance; scalar payloads use `data.observation`. Process/runtime generations remain per component; identical
local object/request IDs and clocks get separate prefixes. A restart can be a
new component with a distinct generation and an explicit `restart` link. Never
join two births just because their PIDs or local request IDs match.

Limits: eight components, 64 links, 32 clocks and the shared 8 MiB input, 16 MiB
index, 20,000 rows and 64 KiB row bounds. No recursive bundles. Component
build/architecture must match the outer build; contradictory observed identity,
sample symbol-build mismatches, malformed exports and unknown fields refuse.
Absent runtime/native sides are recorded as incomplete, even if the other side
parses. Imported labels and symbols remain unverified producer claims.

## Export routes and compatibility

| Family | Runtime evidence accepted | Native side and limits |
| --- | --- | --- |
| Kotlin / Compose Multiplatform | Explicit coroutine/recomposition snapshots, Chrome trace or speedscope | Android/Apple/native trace evidence for that build. Shared-code frames keep their producer symbols; no JVM agent is injected. JVM DebugProbes availability does not imply Android or Kotlin/Native availability |
| .NET MAUI | Sampled or evented speedscope exports from the selected diagnostic tool; explicit managed exception/GC snapshots | Native logs/reports/traces remain separate. Runtime diagnostics/router must be configured outside this source; no EventPipe client, nettrace decoder or managed debugger is supplied. AOT/trimming can remove observability or symbols |
| Capacitor / Cordova / Ionic / hybrid shells | Selected embedded CDP window, browser operation export, Chrome trace or explicit snapshots | App shell and each page/process are independent. Bridge links require actual IDs/witnesses |
| Electron / CEF / WebView2 / compatible Tauri host | Same CDP route when that host already exposes a compatible page endpoint; otherwise supported exports | Renderer evidence does not establish main/utility/native process behavior. No debugging flag or host configuration is enabled automatically |
| WKWebView / WebKit-backed Tauri | Explicit WebKit/app snapshots, compatible browser-operation or Chrome trace exports | WKWebView does not speak CDP; a WKWebView bundle declaring embedded CDP refuses. Native Apple evidence and existing WebKit owners retain their boundaries |
| Unity / Unreal / custom engine | Explicit scripting/native/asset/CPU/GPU snapshots, Chrome trace or speedscope exports | Native evidence retains build/architecture. Unity binary profiler data and Unreal `.utrace` require their own explicit exporter; Testrun neither decodes nor runs those tools |

These are fixture-tested parser/transport contracts, not certified live SDK,
runtime-version or deployment combinations. Evented speedscope requires balanced
open/close events with valid frames and ordered axis positions. It emits
exclusive leaf intervals; sums are not CPU utilization or GPU completion.

`runtime-samples-v1` is Testrun's explicit interchange, not a vendor wire format.
Populate it from an app/exporter that actually exposes the observation. Its
`runtime` is `kotlin`, `dotnet`, `unity`, `unreal`, `custom`, `javascript` or
`webkit`. Records have `name`, `category`, `identity`, `frames` (up to 128),
optional `state`, optional `actionId` and optional `value`/`unit`/`definition`
as a complete triple. Up to 4,096 records. Categories are `coroutine`,
`recomposition`, `managed_exception`, `gc`, `scripting`, `native`, `asset`,
`cpu`, `gpu`, `bridge`, `navigation`. Coroutine state is required:
`created`, `running`, `suspended`, `completed`, `unknown`. Do not infer duration,
allocation, complete live coroutine enumeration, GPU/display equivalence or leak
ownership from a snapshot. `symbolsBuildId` may be absent; it must match `buildId`
when present and still does not authenticate symbol contents.

Target `class` accepts phone/tablet/desktop/tv/watch/automotive/spatial/extension/
service/unknown; `os` accepts android/ios/macos/windows/linux/tvos/watchos/visionos/
unknown; `transport` is adb/apple/cdp/import. Inconsistent OS/transport declarations
refuse. These fields describe the evidence source, and grant no target input
driver or native source support. If the selected class lacks an actual available
collector, use explicit exports. No watch, TV, automotive, spatial or extension
automation is implied.

If no page is discovered, check the selected endpoint and build's deliberate
debugging configuration; do not scan or switch targets. If a report refuses,
select a smaller supported export and matching symbols/build, then import again.
Use [native traces](diagnostic-native-traces.md), [Android](diagnostic-android.md),
[Apple](diagnostic-apple.md) and [evidence assertions](diagnostic-testing.md)
for their respective acquisition and verification contracts.
