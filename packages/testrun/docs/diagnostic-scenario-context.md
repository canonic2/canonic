# Background, media, energy and device context

Use `diagnostics capabilities` and its `scenarioContext` catalog to choose a
source. Keep app/process generation, device/hardware, action, clock, source
coverage and probe effects with every observation. A device measurement is not
an app measurement; player state, advancing position, decoded samples and audible
speaker/headphone output are separate facts.

## Select evidence

| Question | Available source and limits |
| --- | --- |
| Jobs/services/background restrictions | Existing Android state `scheduling` and `power` groups: selected-user services/jobs/alarms, standby/app-op, Doze/wakelock fields; native logs/exit reports remain separate evidence. Imported lifecycle markers cover suspension/tasks/notifications/lock transitions explicitly. |
| Route/focus/session/buffer/decoder | Optional Android state `media` reads fixed `dumpsys audio` and `media_session`. Only lines with exact app plus selected user/UID survive; unscoped following lines are not assigned to that app. Typed app/platform media imports preserve route/focus/interruption/decoder/buffer fields. |
| Playback versus output | Existing `media_status`, volume and explicitly selected `audio_probe` keep their connected-target/controller/cleanup contracts. Android's service-extrapolated position is not decoded or audible progress. iOS playback-state reads remain unavailable. |
| Video/camera/Bluetooth/location/sensors | Explicit app/platform `scenario-context-v1` peripheral markers with effects and hardware class. No active peripheral probe, radio connection, camera/microphone request, location setting or external-device input is started. |
| Battery/thermal/power/energy | Existing Android device battery/thermal snapshots and explicit source-defined measurements; process energy requires declared app/process identity plus attribution evidence. Battery percentage/temperature/status never supply precise app energy. |
| Device configuration | Optional Android state `device_context`: device locale/timezone properties and selected-user font scale/user rotation/auto-rotation readback. Appearance/permissions/per-app locale and other platform settings retain their existing or deferred interfaces. |
| Setting restoration | Explicit before/applied/current ledger, ownership declaration, receipt reference and restore outcome. The query reports candidates/failures; only the existing setting owner can perform an authorized restore with fresh readback. |

See [Android state](diagnostic-android-state.md), [Apple diagnostics](diagnostic-apple.md),
[playback/audio](playback.md), [network context](android.md#network-control) and
[investigations](debugging.md). Apple AVAudioSession route/interruption, native
background lifecycle and peripheral callbacks need app/platform evidence; this
workflow does not install instrumentation or infer them from arbitrary log text.

The optional Android groups add only fixed reads:

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id context --source '{"kind":"android_state","targetId":"android:SELECTED-SERIAL","appId":"com.example.acme","userId":0,"groups":["scheduling","power","media","device_context"]}'
```

Existing target/app/user/boot/process checks, command/output/deadline limits and
partial-capture semantics apply. Defaults remain process/memory/power; media and
device_context must be selected. OEM/permission/format gaps stay unavailable.
Media dump reads do not start the playback helper, audio capture or route changes.
Locale/timezone properties can differ from an app's effective configuration.

## Import and query

```sh
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id scenario --source '{"kind":"import","path":"/absolute/private/acme-context.json","format":"scenario-context-v1"}'
testrun diagnostics analyze diag-RETURNED_REFERENCE collector-RETURNED_REFERENCE --submission-id scenario-analysis
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"scenario","scenario":{"operation":"media","actionId":"play1"}}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"scenario","scenario":{"operation":"energy"}}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"view":"scenario","scenario":{"operation":"restore"}}'
```

MCP `diagnostics_query` uses those filter fields as arguments. `scenario.operation`
is summary/timeline/lifecycle/media/peripherals/energy/settings/restore. Optional
`actionId` filters exact recorded correlation IDs; optional nested `clockDomain`
selects one source clock. Timeline refuses multiple clocks and sorts available
timestamps in the selected original unit; untimed rows remain last without an
inferred event time. It does not align separate native/app clocks or prove cause.
Ordinary kind/time/native/memory/performance/network filters are refused for this
view; ordinary row queries remain available separately.

Rows paginate with cursor/limit (1–100) and a 256 KiB page budget. They retain source
pointers, original measurements, collector/digest, capture outcome, source loss
and unsupported parts. Imports may be deleted after collection; retained queries
do not re-run probes, input, captures or settings operations.

## Scenario exchange, version 1

The strict JSON envelope requires schemaVersion 1, `targetId`, `hardware`
(physical/simulator/emulator/unknown), `coverage`, `clocks`, `records`; optional
`appId` and `processGeneration` are required when any record has app scope.
Clock declarations use seconds/milliseconds/microseconds/nanoseconds without
alignment anchors. Imported identities/hardware/coverage are producer declarations.

Each record has unique `id`, declared `clock`, nonnegative `at`, `scope`
(app/device/external), optional `actionId`, and nested `observation`. Records must
be nondecreasing within each clock; clocks are never ordered against one another.
For example:

```json
{
  "schemaVersion": 1, "targetId": "android:Acme",
  "appId": "com.example.acme", "processGeneration": "boot:42:birth",
  "hardware": "physical", "coverage": "selected app media callbacks",
  "clocks": [{"id": "app", "unit": "milliseconds"}],
  "records": [{"id": "media1", "clock": "app", "at": 100,
    "scope": "app", "actionId": "play1", "observation": {
      "type": "media", "sessionId": "session1", "itemId": "track1",
      "state": "buffering", "positionMs": 200,
      "positionSource": "service_extrapolated", "bufferState": "stalled",
      "route": "producer reported speaker", "audioFocus": "lost"
    }}]
}
```

| Observation type | Required fields; optional fields |
| --- | --- |
| `lifecycle` | `event`, `reason`; optional `resourceId`. Events: process_started/resumed/paused/stopped/suspended/process_exited, job_scheduled/job_started/job_finished/task_expired, foreground_service_started/foreground_service_stopped, notification_posted/notification_removed, wakelock_acquired/wakelock_released, device_locked/device_unlocked. |
| `media` | `sessionId`, `itemId`, `state`, `positionSource`, `bufferState`; optional positionMs, bufferedMs, route, audioFocus, interruption, decoderError, audioSample. State: playing/paused/buffering/connecting/stopped/error/unknown. Position source: player_reported/service_extrapolated/decoded/unknown. Buffer state: empty/filling/ready/stalled/unknown. |
| `peripheral` | `kind` (video/camera/bluetooth/location/sensor), `event`, `resourceId`, `effects`. Effects: camera_active/microphone_active/radio_connection/location_request/sensor_sampling/external_device/unknown; distinct, at most eight. |
| `energy` | `kind`, `value`, `unit`, `attribution`, `definition`; optional `attributionEvidence`. Valid kind/unit pairs: battery_level/percent, temperature/celsius, thermal_status/code, power/watt or milliwatt, energy/joule or millijoule, charge/milliamp_hours. |
| `setting` | `name`, `before`, `applied`, `current`, `phase`, `owner`; optional `receiptRef`, `restoreError`. Names: locale/timezone/appearance/text_size/rotation/permission/connectivity. Values are scalar; null means unavailable for restoration. |

Media position differences require the same scope, clock, session, item and
position source within the declared process generation, increasing sample time
and nondecreasing position. Source/item changes are not joined. A decreasing,
same-time or unknown-source position leaves the delta unavailable; no seek,
stall, decoder failure, process death or audibility is inferred. Service-projected
progress remains labeled. A `process_exited` witness is separate from media state.

`audioSample` requires rmsDbfs, peakDbfs, frames, durationMs, health
(complete/partial/unknown) and routeChanged. Levels are finite −2000..0 dBFS with
RMS no greater than peak. These are imported sample/transport observations,
not app isolation or physical output verification. Actual audio probes retain
their separate explicit route-change consent, duration/clip/cleanup limits.

Energy attribution is device/process/unknown. Device attribution requires device
scope. Process attribution requires app scope, known declared app/process,
power/energy kind and attributionEvidence; it remains a producer assertion.
Battery is 0–100 percent, thermal status an integer 0–6, other energy/charge/power
values nonnegative; temperature may be negative. Values are finite and within
±(2^53−1). No percent-to-joules conversion, power integration or app causality is
derived. Keep source interval/averaging/counter semantics in `definition`.

## Restoration is owned by the setting operation

Setting phase is planned/changed/restored/restore_failed/outcome_unknown; owner
is owned_scenario/external. Non-planned owned entries require a receiptRef;
restore_failed requires restoreError. Labels do not authenticate setter authority.
Scalar strings are at most 1024 bytes. Query `restore` reports only typed ledger
entries, not a restore plan synthesized from passive Android readbacks.

External changes are preserved. Unknown outcomes require inspecting the original
owner. Unknown values cannot produce a restore candidate. A current value that
differs from the scenario's applied value preserves the concurrent change.
Reported restored values are checked against current readback; mismatch and
restore failure remain explicit. A candidate requires a distinct known before/
applied value and current equal to applied, plus fresh authority/readback at the
actual setter before use. The query always reports performed:false.

Use existing network/media/setting operations only within the owned scenario's
authorization. Disconnect does not undo requested network changes. Read current
state and restore only what that scenario changed; do not overwrite a user's later
change. An uncertain dispatched setter must be inspected through its original
action status rather than replayed. Broader locale/timezone/appearance/text-size/
rotation/permission setters and automatic restoration are not registered here.

The shared raw/expanded 8 MiB, JSON/depth/model/row limits and isolated worker
deadline apply; at most 4096 scenario records and 32 clocks are accepted. Live
OS/build/background/audio-route/peripheral/device/energy acceptance and overhead
remain unverified. No active hardware or setting change runs during analysis.
