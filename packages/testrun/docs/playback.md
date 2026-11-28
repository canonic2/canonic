# Playback state, controls and volume

Connect to an explicit target first. Media tools use that device's connected
controller and share its queue and action history with every client. State,
controls and volume do not start capture. An explicit audio probe measures system-output samples.
After installing an updated Testrun build, disconnect and reconnect an existing
controller to load its media support.

Each tool refuses a destination or command the connected platform does not
support with `CAPABILITY_UNAVAILABLE`, before anything is sent:

| Tool | Android | Physical iPhone | Simulator |
| --- | --- | --- | --- |
| `media_status` | Media sessions and playback state | Unavailable | Unavailable |
| `media_send` | `{"kind":"media_session","id":"..."}` | `{"kind":"system_player"}` | Unavailable |
| `volume_get`, `volume_set` | `{"kind":"music_stream"}` or a media session | Unavailable | Unavailable |
| `audio_probe` | Experimental; needs `acceptRouteChange: true` | System audio | Unavailable |

## Android

Use `media_status({})` to list active media sessions, metadata, supported
commands and published playback state. Copy the intended session's `id` from
`sessions[].id` in this read, including when two sessions belong to the same
package or have the same tag. An empty list means no active sessions were
returned; it does not mean every app is paused. Positions are supplied through
Android's service projection path, labeled `positionSource: "service_extrapolated"`;
they are not a measurement of decoded or audible audio. While playing, the
service projects the position forward from the player's last report; a pause
reports the player's own position, which can be slightly earlier than the last
projection, so compare positions with a tolerance.

```sh
testrun connect --target android:SERIAL
testrun media status
testrun media send pause --media-session SESSION_ID --observe-for-ms 1000
testrun volume get
testrun volume set --value 5
testrun volume set --media-session SESSION_ID --adjust down
```

`media_send` takes `destination: {kind: "media_session", id: "SESSION_ID"}`
and a `command`. Android commands are `play`, `pause`, `play_pause`, `next`,
`previous` and `stop`, subject to the chosen session's native actions; volume
buttons go through `volume_set` with `adjust`. `play_pause` reads state and
selects explicit play or pause; unknown/buffering state is refused. Prefer
explicit play/pause when the desired state is known. Sessions without a published
MediaSession cannot be controlled by this tool.

`volume_get` and `volume_set` take `destination: {kind: "music_stream"}` or
`{kind: "media_session", id: "SESSION_ID"}`; the CLI uses the music stream unless
`--media-session` is given. `volume_set` requires exactly one of integer `value`
or `adjust: "up" | "down"`. Native indices use the returned min/max range, not
percentages. Fixed-volume targets are refused; relative-only targets cannot
accept an absolute index. Remote session volume is independent of the phone's
music stream. Platform rules can link local session and stream volume and can
affect mute when setting an index.

To prove that a downloaded track plays without its stream, take the target
offline first with `network_set({mode: "offline"})`, start playback, and read
`media_status` while the result's `after.connectivity` is `offline`. Request
`network_set({mode: "online"})` explicitly when finished; disconnect leaves
network settings as requested. See
[network control](android.md#network-control).

Testrun starts one bundled helper through the selected target's adb shell. It
does not install an app or request notification access. The current adapter
requires Android 12/API 31 or later, shell media permissions and compatible
framework initialization. An unsupported OEM/runtime reports a setup failure;
it does not silently fall back to text commands or global media keys. The helper
is cleaned up with the controller. Java and the Android build tools are needed
to build Testrun, not to use these tools.

## Physical iPhone

`media_send` with `destination: {kind: "system_player"}` sends one system
media button through CoreDevice Indigo, without an XCTest runner or an added
audio stream. iOS chooses the receiving player, which can be in the background.
The response cannot identify that app.

```sh
testrun connect --target device:SELECTED-UDID
testrun media send pause --system-player
testrun media send volume_up --system-player
```

MCP arguments are `{"destination":{"kind":"system_player"},"command":"pause"}`.
Commands are `play`, `pause`, `play_pause`, `next`, `previous`, `volume_up` and
`volume_down`; `observeForMs` is refused because iOS cannot report the effect.
Relative volume steps may vary or coalesce; previous can restart the current
item according to the player's behavior. Compatible physical CoreDevice
developer support is required. Simulator media control, iOS playback
state/metadata and absolute volume reads/settings are unavailable.

## Measure system audio

```sh
testrun audio probe --duration-ms 2000
testrun audio probe --duration-ms 2000 --accept-route-change
```

The matching MCP tool is `audio_probe` on a physical iPhone or Android device.
`durationMs` is 100..10000 (default 2000); `retainClip` defaults to false.
Android requires `acceptRouteChange: true` (CLI `--accept-route-change`) on every
call. Its experimental remote-submix capture changes output routing and captured
policy-restricted fixture audio on the lab-tested Android 12 phone. Newer
versions may reject it. Recorder failure does not disable media state, controls
or volume.

Results report sample counts, decoded duration, RMS/peak dBFS and digital-zero
counts, plus transport/decoder health and cleanup. The backends measure the
system mix before output volume. `sampleCount` counts interleaved channel samples;
`frameCount` counts stereo frames. UTC observation bounds include native setup
and cleanup; `decodedDurationMs` measures the audio interval. They do not identify
an app or prove audible
speaker/headphone output. iOS AirPlay was not captured in the lab; zero samples
cannot identify the route. Integrity is `not_fully_verifiable`, including when
all samples are zero. Loss, decode failure and missing progress are errors.

No clip is written by default. `--retain-clip` / `retainClip: true` saves private
stereo 48 kHz signed 16-bit little-endian PCM and a JSON manifest; the result
contains local paths, never an automatic audio attachment. Audio clips are
bounded to 16 MiB. iOS audio release takes about 3 seconds after measurement
while CoreDevice expires the audio session; with a connected controller's video
running, a 3-second probe returns in about 6 seconds. Video and HID remain
independent.
The full call has a 75-second budget. Shipping MCP checks on one physical iPhone
and one Android phone captured two seconds of digital silence and verified cleanup;
the Android phone's reported speaker route matched before and after capture. These
checks establish transport/decoder operation on those devices, not positive
audio delivery or compatibility with every OS or route.

## Results and interrupted calls

A control/volume result with `dispatch.state: "sent"` establishes native
transmission, not a player acknowledgement or audible output. iOS media controls
return `observed: false`.
Android optionally compares before/after state for `observeForMs` (0..2000,
default 0). Its effect can be `not_checked`, `state_changed`, `no_change` or
`unavailable`. Concurrent user/system activity can also cause a change.
`play` and `pause` end when the session reports the requested state. `next` and
`previous` end when the track changed, or `previous` restarted it, and the player
left its buffering, connecting and skipping states, naming the new track when it
named the old one; `observed: true` then means that settled skip was seen. A
restart reports `state_changed`, though only the position changed. An
observation that ends first reports the last state read, so give a slow-loading
player a longer `observeForMs`.

Admitted mutations return an `actionId`. Inspect an interrupted call with
`status({actionId: "media-REFERENCE"})` on either platform, or CLI
`testrun status --action-id media-REFERENCE`. Normal status includes
`lastMediaOperation`. A session `id` from a stopped/restarted Android helper is
stale; read state again before choosing a session.

Testrun does not automatically repeat a dispatched media command. If an outcome
or iOS release is uncertain, inspect status and current evidence. Disconnect and
reconnect before further mutations only after disconnect confirms cleanup.
Status is read-only. After `CLEANUP_FAILED`, Testrun retries the release itself
and `status --target` reports `state: "releasing"` until it finishes; other
devices are unaffected. A new explicit call sends a new command.
Neither playing state nor a moving position proves physical audibility.
