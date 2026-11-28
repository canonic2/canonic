# Android QA

This guide covers Android devices and emulators. The tools, commands and
recovery rules are the same as on iOS and are described in the
[iOS guide](ios.md); this guide covers what differs on Android.
[Playback and volume](playback.md#android) covers native media-session state,
player controls, stream/session volume and interrupted-command recovery.

## Setup and readiness

Testrun uses the Android SDK's adb for explicit device/emulator selection. Put
adb on PATH or set `TESTRUN_ADB` to its absolute executable path. Testrun needs
no configuration file. Run `testrun target list --kind android` for identities
and transport state, and select an explicit discovered `android:SERIAL` target.

### Readiness diagnosis

Discover an explicit target, then diagnose it before connecting:

```sh
testrun target list --kind android
testrun doctor --target android:SERIAL
testrun target list --kind android --probe
```

MCP exposes `doctor({targetId: "android:SERIAL"})` and optional `probe: true` on
`target_list` (add `kind: "android"` to list only Android targets). Diagnosis
works without a connection. It sends no input, changes no debugging settings,
acquires no control and preserves existing connections.

Inspect the returned `ready`, `state` and `checks`. Each check reports `ready`,
`blocked` or `unknown`, with a reason, bounded error or recovery action where
applicable. Checks cover adb availability, authorization/offline/absent state,
transport, shell access, Android release/API level, boot completion, screenshots
and accessibility. A blocked or unknown report is a successful diagnostic reply;
invalid arguments fail the command.

Successful screenshot and accessibility checks report dimensions and node counts,
without publishing the image or screen text. Missing properties and empty/partial
hierarchies remain unknown. Earlier results survive a later failure or timeout.
A disconnect or transport replacement during diagnosis prevents `ready: true`.
`usbInstallVerification` reports whether Google Play Protect checks apps
installed over USB, with a warning when it does; it never affects `ready`. See
[Google Play Protect prompt](#google-play-protect-prompt).
The report's scope is `adb_observation`: readiness does not certify unlock state,
app focus or input delivery, and `inputReadiness` remains `unknown`.

Standalone diagnosis has a 25-second total budget with shorter per-probe bounds.
Probed listings share the discovery budget and can return a readiness timeout for
an individual target; use explicit `doctor` for its detailed report. Accept an
authorization prompt or reconnect only the selected target when instructed;
Testrun does not reset the shared adb server or authorize debugging itself.
An adb state other than `device`, such as `offline` or `unauthorized`, leaves
every capability `unknown` until the target is usable.

### Start an emulator from an AVD

`target list` also shows each Android Virtual Device as `android:avd:NAME` with
state `Available`. Testrun finds AVDs in `ANDROID_AVD_HOME`,
`ANDROID_USER_HOME/avd` or `~/.android/avd`, and the emulator through
`TESTRUN_EMULATOR`, `ANDROID_HOME` or `ANDROID_SDK_ROOT`; the entry's
`avd.emulator.state` is `missing` when no emulator executable is found.

Starting is explicit:

```sh
testrun connect --target android:avd:Acme-QA --boot
testrun disconnect
```

MCP uses `connect({targetId: "android:avd:Acme-QA", boot: true})`; add
`window: true` (CLI `--window`) to show the emulator window instead of running
headless. The emulator starts read-only and does not save a snapshot, so apps,
settings and files changed while it runs are discarded when it stops; the
AVD's data and snapshot images are not modified. Testrun waits until the
started emulator passes every readiness check above, then connects to it as
`android:emulator-PORT`. Disconnect, or ten minutes without use from any
client, stops only that emulator. If the manager is killed or crashes, the next
manager stops the emulators its predecessor started when it starts.

Connect refuses an AVD that is already running outside Testrun
(`AVD_ALREADY_RUNNING`, naming its `android:emulator-` target to connect to
instead) and never stops an emulator it did not start. A start can take longer
than one connect request allows. If connect returns `TIMEOUT` or `RECOVERING`,
inspect status for that AVD before further input. Repeat connect with the same
arguments to wait for/reuse that same owned start, or disconnect the AVD to stop
it. A successful connect reports its resolved emulator target; both names route
to the same owned queue and release. `EMULATOR_EXITED` includes the emulator log path.

## Connect and act

`connect`, `act`, `observe`, `status` and `disconnect` work as described in the
[iOS guide](ios.md#connect-and-act), including action lists, `delay`, fast and
evidence profiles, refusal details, recovery and sharing a device between
clients:

```sh
testrun connect --target android:SERIAL
testrun act '{"action":"tap","point":{"x":0.5,"y":0.5}}'
testrun observe --profile evidence
testrun status
testrun disconnect
```

`connect` refuses `controlPath`,
`appId`, `runnerTeamId`, `setup` and `prepareSupport` for an Android target with
`INVALID_ARGUMENT`; Android has no developer-support preparation or XCTest runner.

`observe --target android:SERIAL` connects the named Android device or running
emulator when needed and leaves it connected. The connection uses the normal
adb authorization and ownership
checks. The same behavior applies to [other fresh reads](cli.md#automatic-connections-for-reads).
Starting an AVD requires `connect --boot`. Input commands require an existing
connection.

By default, `observe` returns a fresh `screencap`. Testrun fetches its
raw pixels, about 4 bytes per pixel, and encodes the PNG on the host. Over a
typical USB connection that costs about the same on every screen, while the
device's own PNG encoder is quicker on plain screens and slower on busy ones.
A raw layout Testrun does not recognize falls back to `screencap -p`. Each
Android observation's `captureTimingsMs` names the path taken (`backend`
`adb_screencap_raw` or `adb_screencap_png`), with `transferMs`,
`transferBytes`, the host's `encodeMs` and `totalMs`; a settled observation
also gives `settleSamples`.

### Experimental streaming preference

Source builds expose `connect --android-capture stream`, or MCP
`connect({targetId: "android:SERIAL", androidCapture: "stream"})`. The default
is `screenshot`. This backend is experimental; integrated acceptance is pending.

Streaming is limited to fast observations on a physical API 31 Android device
over USB with a macOS host. It uses an owned H.264 helper and host decoder.
Other platforms, unsupported or unknown capture policy, a changed display,
failed recovery, or a source that produces no qualifying new buffer use fresh
screenshots. Static observations can wait 500 ms for a new source buffer before
falling back. Evidence and settled observations use screenshots. The setting
does not enable preview or recording streams.

The preference belongs to the connected device, so every client uses it.
Omitting the setting when reconnecting retains the active preference. An explicit
change while the device is connected returns `SESSION_SETTINGS_CONFLICT`;
disconnect first. `status` reports
the stream state and reason. Observations report the effective backend and
requested state in `captureTimingsMs.androidCapture`; stream output names
`adb_mediacodec_h264`, its generation, source sequence and request barrier.
`captureAgeKnown: false` means absolute source-frame age is unknown. A buffer
acquired after a request barrier is not a measured physical frame-age bound.

Confirmed protected windows or composed secure layers block the observation;
unknown policy disables retained video. Stream cleanup must finish before a
recording starts, after which observations stay on screenshots until disconnect
and a new connection.
`CLEANUP_FAILED` leaves the device connected with its release unfinished;
Testrun retries the release automatically, and other requests to the device
return `RELEASE_PENDING` until it finishes.
`CAPTURE_TIMEOUT` means the host stopped waiting for video while its worker
continues toward release. While status reports `releasePending: true`, reads
return `CAPTURE_PENDING`. Retry observation after it clears; successful release
enables raw fallback without disconnect.

After input, `act` repeats `screencap` until the picture stays still for 300 ms,
up to 1.5 seconds, and returns the last one, so screen transitions such as Back
are not caught mid-animation. Without a pre-input picture, the observation's
`settle` reports `changeDetected: null` with `settled`, `waitedMs` and
`samples`; an animation that starts after the first still pair can still be
missing. `settle: false` uses the selected fast backend, with fresh screenshot
fallback when streaming cannot qualify a source buffer.

Android returns no screenshot while the app in front marks its window as
secure, as some streaming, banking and password apps do. Testrun then reports
`CAPTURE_BLOCKED` with the app's package instead of a capture failure. In
`act`, that observation error leaves the batch's status as it was: its actions
ran, and only the picture is missing. `connect` also succeeds while such an app
is in front, with the blocked screenshot as the connection's
`observationError`. Accessibility reads still work, so check
such screens with `ui_find` or `wait`. A capture that returns nothing for
another reason reports `CAPTURE_FAILED`.

Gestures map normalized points using current screenshot dimensions, including
landscape rotation and display-size overrides; geometry changes refuse stale
input. Commands use bounded adb processes and quote each shell argument.
Gesture durations accept integer 0..10000 ms; zero requests no intentional
duration while adb and the device still impose processing time. App- and
system-scoped input, targeted append, replacement without an element, launch
arguments, unsupported adb text and keys outside the catalog fail before
admission, so no earlier action in the request runs.

`open_url` sends one VIEW intent through `am start`; Android reporting no handler
fails the action.

If disconnect returns `CLEANUP_FAILED` (with `details.retrying: true`), the
device stays connected with its unfinished release, and
`status --target android:SERIAL` reports `state: "releasing"` with the
`release` attempts and error. Testrun retries the remaining helper or evidence
cleanup itself, two seconds after the failure and then at growing intervals up to
every 30 seconds, skipping completed stages and keeping the action history.
Resolve a reported prerequisite, such as `details.nextAction:
"reattach_device"`, and the next attempt finishes the release; a new `connect`
to the device retries it first. `testrun manager retry --target
android:SERIAL` runs the attempt at once. Other devices are not affected.

### Keys

Android accepts the shared key names from the [iOS guide](ios.md#typing-and-keys);
`home` and `end` move the text cursor to the start and end of the line. Android
also accepts `home_screen` (navigation Home), `back`, `app_switch`, `volume_up`,
`volume_down` and `power`. Key chords use `input keycombination`. Connect reads
the target's `input help` once and reports the result as
`capabilities.key.modifiers`; on a target without `keycombination`, a chord fails
before admission.

### Typing

`type_text` sends printable ASCII through adb's input utility; literal `%s` is
refused because that utility interprets it as spaces. One action types at most
2,000 characters; split longer text across actions, or set a field's whole
value with `replace: true`. Without `timeoutMs`, the action's deadline is the
larger of 30 seconds and 15 seconds plus 50 ms per character, so 2,000 characters
fit in one `act` call.
Text goes in ordered commands of up to 32 characters, and the result reports how many in `chunks`.
Each command stamps all of its key events with the time it starts, and Android
drops events that are still queued 10 seconds after that time, so a single
command for long text loses its end. If a command fails after earlier ones
were sent, or takes 8 seconds or more, typing stops with `OUTCOME_UNKNOWN`
and reports `sentCharacters` and `textCharacters`; read the field before
typing again. A completed result means every command finished, not that the
app accepted the text; read the field back after long input. Typing checks the active
window's input focus directly through Android automation before sending text.
This bounded probe works without waiting for global UI idle, requires Android 7 /
API 24 or later, and leaves other accessibility services enabled. It requires a
fresh, focused, editable, enabled and visible field in that window and returns
no field contents. If that read fails, the action reports
`FOCUS_VERIFICATION_UNAVAILABLE` with `inputDispatched: false` and the inspection
cause; a readable tree without a focused editable field reports
`FOCUS_NOT_PROVEN`. These are known failures, not unknown text delivery. A fully
recorded focus refusal with successful observations preserves the controller.
Run `doctor` and inspect the current screen before choosing another action;
Testrun does not bypass focus verification or replay the refused input. The
probe exits after eight seconds on device; the host call is bounded to twelve
seconds including setup and cleanup.

For Unicode or replacement, use `type_text` with `replace: true` and an explicit
`element` query. The uniquely resolved field must already be focused and
enabled; Testrun does not tap it implicitly. Empty text clears it.

```json
{"action":"type_text","element":{"query":{"identifier":"com.example.acme:id/search"}},"replace":true,"text":"café 日本語 😀"}
```

This sets the field's text directly through accessibility, with the cursor at
the end. It does not exercise IME composition, autocorrection or per-key app
handlers. The helper revalidates the complete active-window tree before the
single mutation. Changed/partial trees refuse; uncertain delivery is never
replayed. Text is limited to 64 KiB UTF-8 and NUL is refused. Secure values remain
omitted from results. A `{snapshotId, elementId}` element is refused for
replacement; use a fresh query.

### Element targets

`tap` and `long_press` accept `element` instead of `point`, using either
`{query: ...}` or `{snapshotId, elementId}` from a read. Resolution takes a fresh
tree before dispatch; changed trees refuse stale references. Multiple matches
return bounded labeled choices. Truncation cannot prove uniqueness, and absence
requires complete coverage.

## Reading the screen

`ui_snapshot`, `ui_find`, `ui_read` and `wait` take the arguments described in
the [iOS guide](ios.md#reading-the-screen), with `scope: {"kind":"display"}`:

```sh
testrun ui read '{"query":{"identifier":"com.example.acme:id/home"}}'
testrun wait '[{"kind":"foreground","appId":"com.example.acme"}]' --timeout-ms 5000
```

Android tree coverage is `active_window_exposed_tree`: complete means the exposed
active root was fully traversed, not that every app window or off-screen item was
observed. Node, depth and child-fetch limits, missing children or a window
change mark the result partial. Long text is cut per element instead: a text
field keeps up to 65,536 characters, other text and descriptions 1,024, and a cut
element reports `valueTruncated` or `labelTruncated`. A search whose answer
depends on the missing part reports `truncated` rather than a match or a miss. Reads are best effort while the UI changes;
they do not freeze the screen. Partial reads require `allowPartial: true`; no
match on a partial tree is inconclusive. Secure values are omitted from reads and
cannot satisfy value conditions. Direct Android automation supplies bounded
active-window observations on API 24+ without waiting for idle.

Frames are in pixels. `ui_snapshot` and `ui_find` return `display`, the
current display size that taps scale to, which follows rotation; it is read
from the input service's viewport, with a screencap only as a fallback. Each
`ui_find` match centered on the display has `center`,
the `{x, y}` fraction that `tap` takes.
When the display size cannot be read, `display` is null and matches omit `center`.

Roles use the shared vocabulary (`text_field`, `checkbox`, `switch`, `slider`,
`image`, `scroll_view`, `button`, `text`), keep the widget class as
`nativeRole`/`class`, and report frames in pixels.

`wait` states are `enabled`, `focused`, `selected`, `checked`, `clickable` and
`scrollable` where exposed; missing state is unknown. Value and state conditions
require one unique element. A `foreground` condition compares the focused
window's package with `appId`.

Android has no clipboard adapter; `clipboard_read` is refused with
`CAPABILITY_UNAVAILABLE`.

## Apps

`app_launch`, `app_terminate` and `app_restart` take a package name as `appId`.
Restart runs `am force-stop`, then launches the package's launcher activity with
`am start -W`. Add `readiness` to wait until the app's screen is shown:

```sh
testrun act '{"action":"app_restart","appId":"com.example.acme","readiness":{"element":{"identifier":"com.example.acme:id/home"}}}'
```

`readiness: {}` waits until the focused window (`dumpsys window`, `mCurrentFocus`)
belongs to the package's activity; a starting window or system dialog does not
count. `element` also waits for a matching active-window element. Stages and
`APP_NOT_READY` are reported as in the [iOS guide](ios.md#launch-terminate-and-restart).
Launch arguments are refused. Restart never clears app data or uninstalls the app.

`app_list` returns up to 20 apps plus a private JSON file of every app it
selected. Each app has its `appId`, the `name` shown under its home-screen icon
(which can differ from the package name), `version`, `build` and `launchable`.
By default only apps with a home-screen icon are listed, and `hiddenNotLaunchable`
counts the rest; `all: true` lists every package. `name` searches the name and
app ID with a text matcher, such as
`{"text": "quest", "mode": "substring"}` (CLI `app list --name quest`). The
names come from one package-manager read on the device; if that read cannot
run, the list falls back to package IDs and version codes, names `name` and
`version` in `unavailableFields`, and gives the cause in `namesUnavailable`.
Cancelling a read lets its bounded helper operation finish and remove its files.
A helper-removal failure returns `CLEANUP_FAILED` instead of the fallback list.

`app_install` installs a standalone APK from an absolute host-local `path`, with
`replace: true` for an update, and `app_uninstall` removes a package by `appId`.
Both use the only connected device unless `targetId` is given; a named device
that is not connected is used once, through its queue, without connecting it:

```sh
testrun app install --target android:SERIAL /absolute/staging.apk
testrun app install --target android:SERIAL --replace /absolute/staging.apk
testrun app uninstall --target android:SERIAL com.example.staging
```

Testrun uses non-streaming adb installation and never uninstalls first, grants
permissions or enables downgrades to fix a rejected update. Installation needs
Android SDK Build Tools `aapt2` to inspect APK identity; Testrun finds it in the
SDK associated with adb, `ANDROID_HOME` or `ANDROID_SDK_ROOT`. `TESTRUN_AAPT2`
can select the exact executable. Split APK sets and `.aab` files are unsupported.
Uninstall follows Android's normal app/local-data removal rules. Foreign
controllers remain untouched.

Results include verified package/version observations and an `actionId`. Inspect
an interrupted call with `status({actionId: "app-REFERENCE"})` or
`testrun status --action-id app-REFERENCE`; status reports the kept outcome plus
a bounded current-app observation, without replay. Validate data retention on
the devices and app versions you intend to test.

When an install's outcome is uncertain, the error and later `status` include
`reconciliation`: `targetBuildInstalled` is true only when the installed base
APK's SHA-256 matches the submitted package. It is a current observation; the
kept outcome stays unknown.

### Google Play Protect prompt

Some devices verify apps installed over USB. Google Play Protect can then show
"Send app for security check?" with **Send** and **Don't send**, and the install
waits until someone answers. `doctor` reports this setting as
`usbInstallVerification`.

While an install runs, Testrun watches for a Play Store window taking focus.
By default (`playProtect: "fail"`) it stops waiting within a few seconds and
returns `INSTALL_AWAITING_PLAY_PROTECT` with the prompt's `buttons`. It does
not tap anything. The device may still finish the install once someone answers,
so the outcome is unknown: check `reconciliation` or `status` before retrying.

This prompt is a security decision on the device, so answering it is opt-in.
Choose the answer explicitly:

```sh
testrun app install --target android:SERIAL --replace --play-protect dont_send /absolute/staging.apk
```

MCP takes `playProtect: "dont_send"` or `"send"`. Testrun taps the button with
that English label and records it under `playProtect.answers` in the result.
On a device in another language the label does not match. The install then
returns `INSTALL_AWAITING_PLAY_PROTECT` with the buttons it found; answer it on
the device or with `act`, then check `status`.

## Capture

Screenshots use `adb screencap` with `fast` and `evidence` profiles; fast images
are converted to compact JPEGs on the host.

Android has no live preview; `preview_start` is refused with
`CAPABILITY_UNAVAILABLE`. `recording_start` runs one owned `screenrecord` into a
uniquely named file under `/data/local/tmp`. Stop interrupts only that process,
copies the MP4 into private run evidence, removes the remote file and verifies
the container (and, on macOS, decodes it). A finished recording reports
`status: "complete"` and `backend: "adb_screenrecord"`. Targets whose
screenrecord cannot remove its time limit record at most 180 seconds; the start
result reports `targetMaxDurationMs`, and a longer `maxDurationMs` is refused with
`INVALID_ARGUMENT` and `details.targetMaxDurationMs`. Recordings have no audio.

An unsuccessful `screenrecord` exit fails the recording and keeps any copied
partial MP4 as evidence. Stop and disconnect await bounded cleanup, including
after a worker timeout; cleanup failures remain visible in the recording result.
If copying/finalization takes more than ten seconds, stop returns `finalizing`
and keeps copying instead of aborting. Poll `recording_status` or
call stop again. Disconnect reports pending cleanup until finalization settles.
The copy keeps its sixty-second bound; a pending result is not a completed MP4.
If the video is saved but its manifest cannot be published, the failure includes
the preserved video artifact so it can still be retrieved.

## Network control

`network_set({mode: "offline" | "online"})` takes the connected target offline or
back online through adb and reads every switch back until the result agrees.
The default `method: "airplane"` uses airplane mode, which also turns Bluetooth
off; `method: "radios"` switches Wi-Fi and mobile data instead and keeps
Bluetooth. Optional `timeoutMs` (1000..60000) bounds the readback; offline
settles in a few seconds, online waits for Wi-Fi to reconnect. Non-Android
targets refuse it with `CAPABILITY_UNAVAILABLE`.

```sh
testrun connect --target android:SERIAL
testrun network set offline
testrun network set online --method radios --timeout-ms 20000
testrun disconnect
```

Each request reports the switches read in `before` and `after`; `status` keeps
the last network operation. A request the readback already satisfies sends no
switch commands and reports `changed: false`.

Settings stay as requested across disconnect, reconnect, server exit and task
end. To return online, explicitly call `network_set({mode: "online"})` while
connected. You can change methods between requests. Online means connectivity
is available; it does not restore every radio to an earlier setting. In
particular, the airplane method disables airplane mode but may leave Wi-Fi or
mobile data off. The radios method can enable those switches when the target
is offline; a request that already agrees leaves individual radio settings alone.
A readback that does not agree within the timeout fails as `NETWORK_UNSETTLED`
with the last readback. After an unknown dispatched outcome, inspect status,
disconnect and reconnect before issuing a new recovery request.

The capability is `network` in discovery. Wireless debugging targets report it
unsupported, because going offline would sever their own control. Connect
reads the API level: airplane mode needs Android 11 (API 30), the radio
switches Android 5 (API 21). The capability's `methods` lists the available
choices. On API 21–29, pass `method: "radios"` explicitly; omitting it selects
airplane mode.

## Android specifics

Discovery reports operation keys with their limits, such as `tap`, `key`,
`type_text`, `open_url`, `app_management` and `recording`. Android targets have no
preview or clipboard reads. [Diagnostic investigations](diagnostic-android.md)
collect bounded app logs and failure evidence independently; [resource snapshots](diagnostic-android-state.md)
collect scoped state and time series. See [automatic connections for reads](cli.md#automatic-connections-for-reads)
for commands that can connect the target they name. The
[retained socket transport](ios.md#retained-socket-transport) works as on iOS.

```sh
testrun observe --target android:emulator-5580
testrun ui snapshot --target android:emulator-5580
testrun ui find --target android:emulator-5580 --role button --label Continue
testrun app list --target android:emulator-5580
testrun act --target android:emulator-5580 '{"action":"app_restart","appId":"com.android.settings","readiness":{}}'
testrun act --target android:emulator-5580 '{"action":"open_url","url":"https://example.com/acme"}'
```

Retained socket batches refuse preview and recording steps on Android with
`CAPABILITY_UNAVAILABLE`.
