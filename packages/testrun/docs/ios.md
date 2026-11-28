# iOS QA

This guide covers iOS simulators and physical iPhones. Testrun uses the same
tools and commands for every platform; this guide describes them in full, and
the [Android guide](android.md) covers what differs on Android.
[Playback controls](playback.md#physical-iphone) covers physical system-player
buttons and audio probes.
[Apple diagnostics](diagnostic-apple.md) covers retained simulator/physical logs,
installed observations, exported crash/Jetsam/MetricKit reports and explicit
UUID-checked dSYM symbolication through the shared investigation tools.

## Setup and readiness

Install with `cargo xtask install` from `packages/testrun`. On macOS with Xcode
selected, run `testrun target list --kind simulator` or
`testrun target list --kind device` to discover simulators and USB devices.
Select the complete `targetId`; names and discovery order never select a device.

Testrun needs no configuration file. All discovered simulators and devices are
available for QA. Select the intended target explicitly; Testrun uses its existing
network connection without network isolation or transmission approval flags.
`network_set` is Android only; an agent that takes a physical iPhone offline
through Control Center gestures with `act` judges the result from `observe`
frames and restores the switch itself. USB control and continuous video survive
Airplane Mode.

### Discovery

`testrun target list --compact` shows names, platforms, models, OS versions and
USB/network transport. Network discovery does not imply network control. Each
target reports `platform` (`"ios"` or `"android"`), `kind` and `capabilities`.
Add `--probe`, or call `target_list` with `probe: true`, for physical-device
readiness. Without `kind`, `target_list` lists every platform and reports a
platform it could not list under `unavailable` as `{kind, error}`; pass
`kind: "simulator"`, `"device"` or `"android"` to list one.

Discovery probes share a bounded budget. A slow check returns
`READINESS_PROBE_TIMEOUT` on that target while preserving the list and completed
checks. Use `doctor` on that explicit target for a dedicated readiness check.

To use a custom simulator device set, set `TESTRUN_SIMULATOR_DEVICE_SET` to its
absolute existing directory before discovery and use the same setting for later
CLI calls or the MCP server. Full discovery reports `deviceSetPath` and
`deviceSetIdentity`; custom-set target IDs include that identity. Copy the whole
target ID from the list. A default-set UUID cannot select a custom-set device.
Path aliases resolve to the same set; replacing its directory changes custom-set
identity. Without this variable, Testrun uses the default CoreSimulator set.

### Physical iPhone readiness

`doctor({targetId: "device:UDID"})` reports pairing, Developer Mode, mounted
support, display/HID services, competing tools and lock state. Unknown checks
stay unknown, with available reasons and next steps. Doctor does not mount images
or enable Developer Mode. CLI: `testrun doctor --target device:UDID`.

`ready: false` also covers unverified readiness. If `checks.lock.state` is
`unknown`, unlock status is unverified. Inspect `checks.lock.reason`; missing
or conflicting lock fields can produce a reason without an error. When present,
`checks.lock.error` describes the lock check's failure. Earlier failures can instead appear in the
top-level `error`, `checks.pairing.error` or `checks.displayService.error`.
Display/HID service checks remain separate. When both services are available,
you can try `connect` on that explicit target to verify live video and input
availability even when the lock probe fails. If either service is absent,
follow `checks.developerSupport.nextAction` first.

`developer_mode_reveal({targetId: "device:UDID"})` reveals the Developer Mode setting.
The user enables it and accepts any phone trust, unlock or reboot prompts.
CLI: `testrun developer-mode reveal --target device:UDID`.

`developer_support_install({targetId: "device:UDID"})` acquires, verifies,
personalizes and installs Cryptex developer support without root or a Python
runtime. Build 27A5228h is cached privately under
`~/.local/share/testrun/developer-support/`; payloads are checked against manifest
SHA-384 digests. Personalization precedes replacement. If the selected iPhone
has no developer image mounted or installed, preparation can run while Xcode
or `xcodebuild` is open. If an image needs replacing, detected Xcode or
`xcodebuild` processes prevent replacement with `DEVELOPER_SUPPORT_IN_USE`.
An active media client on that iPhone also blocks preparation. Disconnect the
phone in Testrun, close the other client, or let the build finish as applicable.
CLI: `testrun developer-support install --target device:UDID`.

Both preparations need `targetId` and run once on that phone through its
request queue without connecting it. On a phone that is already connected they
wait behind the requests ahead of them.

Use `connect` with `prepareSupport: true`, or CLI `connect --prepare-support`,
to prepare once and retry connection on a missing-support error. Preparation has
a ten-minute internal deadline. The initial connection and retry each use the
usual connect budget. After uncertain replacement, inspect doctor before retrying.
Available services do not establish an unlocked phone when the lock probe is unknown.
An incompatible mounted developer image returns `DEVELOPER_SUPPORT_REQUIRED`.

HID testing requires macOS, a paired/unlocked iOS 27+ device and compatible
CoreDevice developer support. The QA host must allow local control sockets and
access to the Mac's USB device service. `HOST_ACCESS_REQUIRED` means the
execution environment denied that access: the integration must request host
execution permission before reconnecting. Repeating connect/disconnect inside
the same restricted environment cannot grant it. This does not establish that
the phone is locked or disconnected. A confirmed device lock or missing USB
connection has its own error and recovery action. Status retains that actionable
failure and its structured cause. Host execution permission is required
separately from optional runner setup.

### XCTest runner setup

Traditional inspection uses an owned signed XCTest runner and USB multiplexing.
Explicit `setup: true` requires Xcode development signing, device trust,
Developer Mode and an unlocked selected device. Choose the default signing team
once per OS user, across projects, with:

```sh
testrun signing setup
```

This command checks local development signing and shows the saved team without
connecting a phone. In a terminal it lets you keep or change the default. To set
the default without a prompt, including from an agent, use:

```sh
testrun signing setup --runner-team-id TEAM_ID --non-interactive
```

Runner preparation reuses the saved team. If none is saved, the first
`connect --setup` (MCP `connect` with `setup: true`) automatically saves the sole
eligible local development team. With multiple teams, the interactive CLI asks
you to choose; MCP and unattended calls return `SIGNING_TEAM_REQUIRED` with
`availableTeamIds` and labeled `availableTeams`. Run the setup command to save a
default, then reconnect. Simulator and HID-only use do not trigger signing setup.
Device trust, Developer Mode, runner installation and profile renewal are
separate from choosing the host's default team.

Use `runnerTeamId` on `connect`, or `--setup --runner-team-id TEAM_ID` with CLI
`connect`, to override the team for that connection only. Overrides do not change
or establish a host default. A saved team with no valid identity returns
`SIGNING_IDENTITY_REQUIRED`; repair signing in Xcode or intentionally change the
default with `testrun signing setup`. Testrun never selects another team silently.
Team discovery reads certificate team metadata, which can differ from the suffix
in an Apple Development identity's displayed name. The picker and
`availableTeams` label each team ID with its team name, the certificate's
organization, as Xcode shows it in Settings > Accounts.

The default lives in `ios.runnerTeamId` in the OS user's
`~/Library/Application Support/Testrun/settings.json` on macOS. Changes apply to
future runner preparations without restarting Testrun;
an active connection keeps its existing runner. Connection results report the
team and its `source` (`saved`, `automatic` or `override`) under
`runnerSetup.signing`. Human CLI results identify the reused team. Interactive signing
selection uses canonic-tui-kit on terminal stdin and stdout: use arrows or numbers
to highlight a team, then Enter to confirm; Escape or Ctrl-C cancels. Lists over
seven teams support filtering. A valid saved team starts highlighted. The live
prompt clears before the result is printed. Human output is the default;
`--json` returns complete structured output and never prompts.
Piped and unattended commands never open the picker. Unattended
signing-selection errors with `--json` include structured choices on stderr;
MCP returns them in its structured error details.

If `SIGNING_PREFERENCE_INVALID` reports an open settings folder, its
`settingsDirectory` identifies the folder and its `recoveryCommand` gives the
program and arguments to restrict it to mode `0700`. Apply that correction to the
named folder, then run `testrun signing setup` again. Ownership and symlink validation
failures include their own `manualAction`; Testrun preserves the folder's
permissions and ownership when refusing setup.

Preparation reuses verified signed products from
`~/.local/share/testrun/signed-runners/` when the runner sources, device, team and
Xcode/SDK/signing identities match. Every reuse checks the product digest, code
signature, profile expiry, authorized device/app/team and signing certificate.
Invalid or expired entries are rebuilt before installation. Builds let Xcode
update provisioning and register the phone with the selected team, so a phone
missing from the team's profile is added during setup; this needs the team's
Apple account signed in to Xcode. Setup reports
`runnerSetup.signing.cacheReused`, the selected team and profile expiry.
Free Personal Team provisioning profiles expire seven days after issuance;
renew signing in Xcode and run setup again when they expire.
[Apple describes Personal Team limits here](https://developer.apple.com/help/account/basics/about-your-developer-account).

Preparation installs the bundled runner and starts its XCTest host. Private
session tokens are passed through a temporary configuration, not process
arguments. The host owns heartbeat and shutdown. The runner shows on the phone
as `TestrunXCTest-Runner`; Xcode adds the `-Runner` suffix. Start the runner
through `connect` with setup; opening its app icon does not establish the XCTest
test-manager session.

The first time XCTest drives a phone's UI, iOS shows "Enter iPhone Passcode for
'XCTest' – Enable UI Automation" and the runner waits until the user enters it
(Settings > Developer > Enable UI Automation does the same). Setup then times out
after 30 seconds; enter the passcode and connect with setup again. Runner setup
failures are listed under [troubleshooting](#runner-setup-failures).

### Simulator boot

Booting is explicit: `testrun connect --target simulator:SELECTED-UUID --boot`,
or `connect` with `boot: true`. A shutdown simulator stays shutdown without this
option. Testrun waits for the platform boot status and then a fresh screenshot;
neither certifies the tested app's readiness. Repeat connect with the same
startup options to reuse that connection. Changing those options replaces the
controller, including shutdown of the previous Testrun boot; starting a
shutdown simulator again still requires explicit boot opt-in.

Releasing the simulator, by `disconnect` or the idle timeout, shuts down a
simulator Testrun booted after its capture and input resources close.
Simulators already booted before connection stay running. A replacement boot
with a different boot stamp is preserved. Cancelled startup keeps its bounded
platform waiter and release record while status, cancellation and other targets
remain usable. A failed shutdown keeps the simulator pooled and is retried
automatically like any [failed release](#release-idle-timeout-and-recovery).
If the manager is killed or crashes, the next manager shuts down the simulators
its predecessor booted when it starts; nobody needs to reconnect them. A boot
whose command failed or timed out returns `SIMULATOR_BOOT_UNCERTAIN` and is
never repeated. Cleanup then checks the simulator's boot time: a simulator that
booted within two minutes after Testrun sent the boot is shut down, and one
booted at another time is someone else's and stays running. Boot does not
authorize erase, reset or closing Simulator windows.

## Connect and act

### Choose a physical control path

| Task | `controlPath` | Observation |
| --- | --- | --- |
| Fast interactive testing | `"hid"` (default) | Fresh JPEG frames from continuous video; `profile: "fast"` |
| Traditional inspection, semantic queries or guarded app input | `"xctest"` with `appId` | Accessibility and full-resolution PNG screenshots; `profile: "evidence"` |

HID starts and keeps video automatically; runner signing/setup is optional. Its
scope is the full main display. HID does not infer app ownership or run XCTest's
app/occlusion admission checks. Observe the intended UI before input. HID sends
keys and US ASCII text to current display focus without verifying an editable
field. App-scoped guarded input and typing with verified editable-field focus
require the XCTest path. A refused or uncertain HID action never automatically
falls back to XCTest.

For XCTest inspection, connect with `--control-path xctest`,
`--app-id com.example.acme` and `--setup` when runner preparation is authorized.
`appId` requires the XCTest path. A HID connection may also request `setup: true`
to enable accessibility, app lifecycle and full-quality evidence captures while
keeping its gestures on HID. `profile: "evidence"` always selects the inspection
screenshot adapter; it does not upscale or relabel a video frame as full-quality
evidence.

### Connect, act, observe, status and disconnect

Select the device once. Testrun keeps the controller and continuous video,
resolves the connection on later calls and keeps interrupted action outcomes:

```sh
testrun connect --target device:SELECTED-UDID
testrun act '{"action":"tap","point":{"x":0.3,"y":0.4}}'
testrun observe
testrun status
testrun disconnect
```

| Tool | Arguments |
| --- | --- |
| `connect` | `{"targetId":"device:SELECTED-UDID"}`; optional `boot`, `controlPath`, `appId`, `setup`, `prepareSupport`, `runnerTeamId` |
| `act` | `{"actions":[{"action":"tap","point":{"x":0.3,"y":0.4}}]}`; optional `observeAfterEach`, `profile`, `settle` |
| `observe` | `{}`; optional `profile: "fast"` or `"evidence"` |
| `status` | `{}`; optional `actionId` from history |
| `disconnect` | `{}` |

Launch `testrun mcp serve` over stdio, initialize MCP and choose a target from
`target_list`. Responses include fresh images directly. Testrun manages
authority, geometry, action identities and recovery itself.

`act` accepts one action or an array of 1..8 actions: `tap`, `swipe`,
`long_press`, `key`, `type_text`, `app_launch`, `app_terminate`, `app_restart`,
`open_url` and `delay`. The selected target can refuse an action it does not
support. Points are normalized from zero to one across the full display. Swipes
use `from` and `to` and optional `durationMs` (default 150); long presses use
`point` and optional `durationMs` (default 500). Explicit durations must be
integers from 0 through 10000 ms. Zero requests no intentional hold or pacing
delay; an HID swipe still sends down, endpoint movement and release in order.
Transport and platform processing still take time. Each action gets a fresh
observation by default; `observeAfterEach: false` (CLI `--final-only`) requests
one final observation. Conditional moves still require inspecting the image
before choosing the next action.

On physical iPhones using HID, bottom-edge swipes can page Control Center or
leave an app open instead of going Home. To dismiss Control Center, observe its
current layout and tap an empty part of its background, away from controls.
To go to the Home Screen, send Command-H:

```sh
testrun act '{"action":"key","key":"h","modifiers":["meta"]}'
```

The `home` key moves the text cursor to the start of the line. Command-H was
verified on physical HID; XCTest does not support key chords. Check the image
after either recovery action. Other overlays may require their own dismiss
control; a background tap is not a universal dismissal action.

Batch only actions whose targets and order are already decided. Insert a `delay`
between inputs when an animation or UI transition needs time before the next
tap; for example, tap, delay 500 ms, then tap. Choose the duration for the UI.
A fixed delay does not prove readiness: when the next action depends on what
appears, send one action, inspect its image or observe, then decide the next action.

`delay` waits a fixed `durationMs` (0 through 60000) between two actions of one
call, without sending input or taking a screenshot. Use it to time a sequence,
such as starting playback, waiting 9.5 seconds and pausing:

```sh
testrun act '[{"action":"tap","point":{"x":0.5,"y":0.8}},{"action":"delay","durationMs":9500},{"action":"tap","point":{"x":0.5,"y":0.8}}]'
```

The delays and gesture durations in one call can total at most 90000 ms. Each
step's `startMs` gives its start time from the beginning of the call. A delay
ends early, failing the call without sending later actions, if control is handed
to a viewer or the connection closes. A delay controls when input is sent, not
when the app reacts, so check the app's own state, such as its elapsed playback
time, to measure what it did. `wait` waits for a screen condition instead.

`open_url` opens a URL through the simulator's URL handler:
`{"action":"open_url","url":"https://example.com/acme"}`. Physical iPhones
refuse it with `CAPABILITY_UNAVAILABLE`.

### Observations

`connect`, `observe` and `act` default to compact `fast` observations on
simulator and physical targets. `observe --target` connects a named running
simulator or uses the default HID connection on a named physical iPhone for
the fast profile. Choose `observe --profile evidence`, or
`act --profile evidence`, for full-quality screenshots; physical evidence
requires a ready inspection runner. MCP `observe` and `act` accept
`profile: "fast" | "evidence"`, including for final-only actions.

On physical iPhone HID, `act` takes a small picture before input and waits up
to two seconds for a detectable visual change afterward. Once change is detected,
it waits for 300 ms of stillness, with a separate 1.5-second limit. No-effect
actions can use the full change window; a transition starting after the window
may still be absent from the returned image. `settle: false` skips the baseline
and both waits.

The observation's `settle` reports `changeDetected`, `settled`, `waitedMs` and
`samples`. `changeDetected: false` means no change exceeded the detector's
threshold within the window; small visible updates can escape detection.
`settled: true` means the sampled picture stayed still, not that the intended
action succeeded. Video, spinners and games can report `changeDetected: true`
and `settled: false`. Check the image or observe again before choosing further
input; an early image does not authorize repeating the action.

Per-action observations compare against the picture before that input. With
`observeAfterEach: false` (CLI `--final-only`), one baseline is taken before the
first input and compared with the end of the sequence, including its delays.
A delay-only observation has `changeDetected: null` because no input baseline
was measured. Baselines are discarded when the call finishes. Fast HID replies
return the video frame checked by the wait; evidence screenshots use the runner
after the video wait. Sampling and capture overhead can extend the nominal
limits. Replies to `observe`, `connect` and `act` summarize the previous action
under `lastAction` without its images; `status` keeps them for recovery.

Fast observations use a JPEG bounded to 960 pixels on the longest edge. HID uses
host-decoded continuous HEVC frames; XCTest fast screenshots resize and encode
at quality 0.55 in the runner before USB transfer. Simulator screenshots apply
the compact conversion on the host. Attachments never upscale the compact image,
and the full display geometry remains valid for normalized input. Fast artifacts
contain the compact observation rather than a full-resolution original; use
`evidence` for detailed visual checks.

### Capabilities

Connect and status report the selected `controlPath` and the target's
capabilities. Operation keys are the tool or action names, such as `tap`,
`long_press`, `swipe`, `key`, `type_text`, `app_list`, `app_launch`,
`app_terminate`, `app_restart`, `open_url` and `clipboard_read`, plus family
keys `app_management`, `accessibility`, `screenshots`, `preview`, `recording`,
`media_status`, `media_send`, `volume`, `audio_probe` and `network`. Launch and
restart list their `readiness` conditions. `unknown` means unverified. Text
details distinguish ASCII, focus verification and replacement support.
Successful adapter checks update the connected device's capabilities, and status
keeps these results for that connection.

### Typing and keys

Named keys are `enter`, `escape`, `backspace`, `tab`, `space`, `delete`, `home`,
`end`, `page_up`, `page_down`, `arrow_left`, `arrow_right`, `arrow_up`,
`arrow_down` and letters `a`-`z`. `home` and `end` move to the start and end of
the line. Key actions accept optional `modifiers` containing `shift`, `control`,
`alt` and `meta`, for example
`testrun act '{"action":"key","key":"a","modifiers":["meta"]}'`. XCTest chords
are unavailable; modifiers never cause an automatic backend switch.

Physical HID supports `type_text` and named `key` actions without XCTest setup.
First tap the intended field and inspect the screen to confirm focus, then send:

```sh
testrun act '{"action":"type_text","text":"Acme.qa@example.com"}'
testrun act '{"action":"key","key":"backspace"}'
```

HID sends keystrokes to the current full-display focus; it cannot prove an editable
field or confine input to an app. Text uses the US ASCII keyboard layout, with
Return and Tab for newline/tab characters. Unsupported characters, element
targets and replacement typing are refused before the action list is admitted.

The first keyboard action registers a controller-owned virtual keyboard and waits
600 ms for attachment and taps Shift once, because iOS can ignore a new
keyboard's first key press. It then types its first three keys more slowly. While that
keyboard is attached, iOS hides the on-screen keyboard, so a person can't type
on the phone. Disconnect before asking the user to enter text on the device,
such as a sign-in or passcode; after a reconnect the on-screen keyboard shows
again until the next text or key action. Later actions reuse it; disconnect
releases keys and removes that controller's keyboard.

The hidden keyboard also changes what QA sees. From the first keyboard action
until disconnect, every screen with a focused field shows no on-screen keyboard,
so these go untested: fields or buttons the keyboard covers and scrolling them
into view, bars above the keyboard, keyboard types such as email or number pad,
the Return key's label, QuickType and password suggestions, and dismissing the
keyboard. Keyboard action results report `hardwareKeyboardAttached: true`, with
`hardwareKeyboardJustAttached: true` on the action that attached it; `status`
reports the same state in `inputHealth.hardwareKeyboardAttached`, and the CLI
prints a notice when the keyboard attaches. To test with the on-screen keyboard
visible, disconnect and connect again with `controlPath: "xctest"`
(`--control-path xctest`), which attaches no hardware keyboard; confirm the
keyboard shows in the screenshot after focusing a field. When keyboard behavior
matters, test both ways.
A single-line field often shows only the end of long text. Before submitting,
send `{"action":"key","key":"arrow_left","modifiers":["meta"]}` to move the cursor
to the start and check the beginning in the returned image.
Interruption attempts key release and blocks further input until reconnect, with
no automatic replay. Completion reports sent input, not verified text entry;
inspect the returned image. Caps Lock, non-US layouts and input methods can change
the resulting text. Checks on one physical iPhone over USB (iOS 27.0.1) verified
case, email punctuation, repeated keys, Backspace and clean disconnect without
XCTest. Validate other layouts and devices before relying on the same behavior.

For XCTest `type_text`, first focus an editable field in the selected app.
The runner queries editable controls, including search fields, then resolves
one enabled, hittable field with keyboard focus. Missing or ambiguous focus
returns `FOCUS_NOT_PROVEN`. A runtime that cannot inspect focus returns
`CAPABILITY_UNAVAILABLE`. XCTest supports Unicode text and accepts an explicit
`element`; replacement typing and IME composition are unavailable on physical iOS.

Simulator typing supports US ASCII, uses proven keyboard focus and rejects an
explicit `element`. Unsupported characters fail before dispatch. Simulator input
targets the full display; app- and system-scoped simulator input is refused.

### Element targets

Simulator `tap` and `long_press` actions accept `element` instead of `point`,
plus `allowPartial: true`. `element` is `{query: {...}}` or `{snapshotId,
elementId}` from an earlier read. They require a fresh unique match or unchanged
element and a full-display accessibility frame mapping; otherwise they refuse
before input. On a HID connection, find a small or crowded target with
`ui_find` and tap the `center` it returns for that match.

### Refused and interrupted actions

Arguments are validated for the whole action list before anything is sent. A
refusal caused by one action names its zero-based `actionIndex` and `actionType`
in the error details and message. For example, refused typing in
`[tap, type_text]` identifies index `1`, type `type_text`; the preceding tap does
not run. A rejected request reports `admitted: false`, `inputDispatched: false`
and `actionsDispatched: 0`, with the allowed range for a numeric bounds error,
such as `actions[0] (swipe).durationMs must be between 0 and 10000; received 10001`.
Correct that request and resubmit; it did not send input. It creates no new
action-history entry and does not relabel an older action. Missing records for
an already-admitted request still mean an unknown outcome.

Errors supply the next normal operation and keep available action results and
images. After interruption, check status and inspect the current screen; uncertain
input is never replayed. HID `reports_sent` does not prove delivery or a UI effect.
A live controller held outside this manager, such as another OS user's Testrun,
is reported as in use and is preserved.

Repeated `connect` reuses a compatible healthy controller. Recovery can close an
uncertain controller and acquire a fresh one without repeating its input.
Status can inspect durable outcomes while an action reply is still pending. A
failed video startup or transient transport acquisition gets one automatic retry
within the same connection deadline. Host access denial is not retried
automatically. After recovery fails, the result explains the next normal action;
verify the selected device is awake/unlocked when live video is unavailable.
Status finds the latest action and up to eight history entries; `actionId`
(CLI `--action-id`) selects one from history. Disconnect is idempotent: with
nothing connected it answers `closed: true`.

For physical HID connections, `status` reports `health.lock.state` as `locked`,
`unlocked` or `unknown`. This read uses the USB tunnel and sends no input. A
missing service, unrecognized response, transport error or 750 ms deadline
reports `unknown` with a reason. Live video and `state: ready` do not establish
an unlocked phone; lock state is separate from input delivery and capture
freshness. Connections without a physical video source report `health.lock: null`.

### Time budgets

Connect and observation recovery each have a 90-second operation budget. Explicit
developer-support preparation uses its own budget. A healthy observation returns as
soon as its fresh frame is ready. Disconnect takes about 5 seconds while
CoreDevice releases Testrun's stream, with a 45-second operation budget. Keep
the connection open between moves. Testrun waits for the release itself; a
successful disconnect permits reconnect without a manual delay.

### Several devices and named targets

Testrun can hold any number of connected devices and browsers. Each `connect`
adds one and keeps the others. Every device tool takes an optional `targetId`
(CLI `--target`):

| Call | Target used |
| --- | --- |
| `targetId` of a connected target | That target |
| `targetId` of a target that is not connected | Refused with `NOT_CONNECTED` and `nextAction: "connect"`, except as listed below |
| No `targetId`, one target connected | That target |
| No `targetId`, none connected | Refused with `NOT_CONNECTED` and `nextAction: "connect"` |
| No `targetId`, several connected | Refused with `TARGET_REQUIRED`; `details.targets` lists them |

A few tools accept a target that is not connected. [Fresh reads](cli.md#automatic-connections-for-reads)
can connect the named device and leave it connected. `app_install`, `app_uninstall`,
`developer_mode_reveal` and `developer_support_install` run once on it through its
request queue without connecting it. Refusals happen before anything is sent.

Work on different devices runs in parallel, including parallel calls from one
MCP server:

```sh
testrun connect --target device:UDID-A
testrun connect --target android:SERIAL-B
testrun act --target device:UDID-A '{"action":"tap","point":{"x":0.5,"y":0.5}}'
testrun observe --target android:SERIAL-B
testrun status
testrun disconnect --target android:SERIAL-B
```

`status` without a target reports the one connected device, or lists several
under `targets`; with `--target` it reports that device. CLI `observe --target`
returns the screenshot and `act --target` the action result directly, and both
need the target connected. A target's `generation` depends only on the device's
connection or boot record, so CLI and MCP name an unchanged device the same way.

### Sharing devices between agents

Every CLI command and MCP server is a client of one background manager per OS
user, which holds every device and browser Testrun uses. Clients do not own
devices. `connect` is idempotent and shared: a device one agent connected is
ready for every client, in any workspace, and any client may send it any
request.

Each device has one queue, shared by every client. Requests that change the
device (`act`, app install and uninstall, `network_set`, audio probes,
recordings, device preparation and `disconnect`) run one at a time in arrival
order. An `act` batch is one queue entry, so another agent's input never lands
inside it. Reads (`observe`, UI reads and `status`) run beside the queue and
never wait for a turn. Agents that share a device share its screen and app
state; Testrun keeps their input from overlapping, and deciding which agent
tests what is up to the user or host.

The one exclusive hold is a person's: `control_acquire` with `owner: "viewer"`
gives a local viewer the device's input. While a viewer holds it, agent input from
every client is refused with retryable `CONTROL_NOT_OWNED`, and reads continue.
Any client's `control_acquire` with `owner: "agent"` takes input back.

Requests carry an optional host label (`TESTRUN_TASK_ID` or `CODEX_THREAD_ID`),
reported in status and history; it grants nothing. Test builds are kept per
workspace: the repository containing the working directory, or the directory
itself. The simulator device set (`TESTRUN_SIMULATOR_DEVICE_SET`) travels with
each request and is part of a custom-set simulator's target ID, so clients with
different device sets address different targets.

Device settings changed on request, such as Android network switches, remain as
left. Action history belongs to the device: `status --action-id` finds an
action whichever client sent it, and history survives disconnect and manager
restarts. Capture artifacts remain private.

### Release, idle timeout and recovery

A connected device stays connected while anyone uses it. It is released when:

- any client's `disconnect` naming it reaches the front of its queue, after the
  requests ahead of it settle. This releases it for every client. A running
  recording is stopped and saved first. `disconnect` with nothing connected, or
  naming a target that is not connected, answers `closed: true`.
- no client has sent it a request that uses it for ten minutes, and it has no
  running operation, recording, preview or viewer holding its input. `status`
  and other inspection do not renew the ten minutes, which pause while the
  computer sleeps.

A client that goes away releases nothing: an MCP server that exits, a cancelled
CLI command or an ended host task only abandons its own requests that have not
started. A restarted agent or MCP server therefore finds its devices still
connected. Browsers close only with `browser_disconnect`, or `disconnect`
naming the browser target.

A release that fails, for example because the phone is locked or unplugged,
keeps the device with its blocker, and Testrun retries the release on its own:
two seconds after the failure, then doubling up to every 30 seconds until the
cause clears. A new `connect` to that device retries first. A `connect` or
`disconnect` that cannot finish the release returns retryable `CLEANUP_FAILED`
with `details.retrying: true` and `details.nextAction`, such as
`unlock_device`, `reattach_device` or `status`. Other requests to the device
are refused with retryable `RELEASE_PENDING`. `status --target` shows
`state: "releasing"` with the release's cause and next attempt. Other devices
are never affected.

`testrun manager` inspects the manager and runs releases now. No recovery needs
a process kill or lock-file deletion:

```sh
testrun manager status       # manager identity, connected targets, idle times, viewers, release retries, operations
testrun manager blockers     # unfinished releases and what they wait for
testrun manager retry        # run every pending release now
testrun manager retry --target device:UDID   # run that device's pending release now
testrun manager operation show OPERATION
testrun manager stop       # stop the manager if no device or client needs it
testrun manager drain        # refuse new requests, release devices once idle, then stop
```

A `retry` answers with an operation; one that does not finish within its
45-second budget answers `pending`, and `manager operation show` looks it up later.
Blockers name their cause and an expiry estimate where the platform gives one;
an elapsed estimate does not prove release. `drain` also closes MCP servers'
connections to the manager. While it drains, device requests return retryable
`OWNER_DRAINING`, `disconnect` answers `state: "closing"` because the drain
releases the device itself, and status keeps answering. Inspection never starts an absent manager; `retry`
starts one only when saved records show an unfinished release.

### Host task end

Host integrations can end a host task with `testrun task stop`, or with the
initialized MCP server's empty `testrun/task_end` request or
`notifications/testrun/task_end` notification. Task end releases nothing and
returns `released: []`; devices stay connected for other clients until
`disconnect` or the idle timeout. The MCP server refuses requests it received
before the task end with `TASK_ENDED`; requests already running finish. Hosts
must not invoke task end for ordinary turn completion, compaction or a
temporary client exit. Testrun does not install agent-specific hooks
automatically.

### The background manager

The first request that uses a device starts the manager; it stops on its own
once no device is connected, no queue has work, no test build is running and
no client connection is open. `status` never starts it: with no manager running,
status reads the saved records and reports `owner: "absent"`. When it runs,
`status` includes its summary under `manager`.

The manager keeps the environment of the command that started it. Tool settings
such as `PATH`, `DEVELOPER_DIR` (which Xcode `xcrun` uses), `ANDROID_HOME`,
`ANDROID_SDK_ROOT`, `TESTRUN_EMULATOR`, `TESTRUN_AAPT2` and `TESTRUN_XCRUN`
apply to every later request until it exits; a command run with different values
does not change them. To apply new values, stop it with `testrun manager stop`
once nothing needs it, or `testrun manager drain`, and the next command starts a
fresh one.

Its records and log (`owner.log`) live in the data root: `TESTRUN_DATA_ROOT`
when set, otherwise `/tmp/testrun-qa-<uid>`. A separate data root runs its own
manager, which still contends for devices with others.

After installing a new Testrun build, restart MCP servers and agents so they run
it. When a client meets a manager from a Testrun build with a different
protocol, it asks that manager to retire if it is empty, or to drain if it is
busy; until it retires, new requests are refused with retryable
`OWNER_UPGRADE_PENDING`. Input is never replayed, and action history remains
available.

## Reading the screen

`ui_snapshot`, `ui_find`, `ui_read`, `wait` and `clipboard_read` use the only
connected device or the one named with `targetId`. On a named running simulator,
[fresh reads can establish the connection](cli.md#automatic-connections-for-reads).

| Tool | Arguments |
| --- | --- |
| `ui_snapshot` | `{"scope":{"kind":"display"},"allowPartial":true}` |
| `ui_find` | `{"scope":{"kind":"display"},"query":{"role":"button"},"allowPartial":true}` |
| `ui_read` | `{"element":{"query":{"label":{"text":"General","mode":"exact"}}},"allowPartial":true}` |
| `wait` | `{"conditions":[{"kind":"exists","query":{...}}],"timeoutMs":5000,"allowPartial":true}` |

```sh
testrun ui find --role button --label Continue --allow-partial
testrun ui read '{"query":{"label":{"text":"General","mode":"exact"}}}' --allow-partial
testrun wait '[{"kind":"exists","query":{"label":{"text":"General","mode":"exact"}}}]' --allow-partial --timeout-ms 5000
```

`ui_snapshot` and `ui_find` take an explicit `scope`: `{"kind":"display"}` on
simulators, and `{"kind":"app","appId":"com.example.acme"}` or
`{"kind":"system"}` on a physical iPhone, where display scope is refused with
`CAPABILITY_UNAVAILABLE` before the runner is called. The CLI's `ui snapshot` and `ui find`
read the display by default, one app with `--app-id com.example.acme`, or the
system UI with `--system`. Simulator reads require `allowPartial: true` because the
native translator exposes the frontmost app rather than a complete system tree.
Queries use `identifier`, `role`, `label` and `value`; text matchers use
`{text, mode: "exact" | "substring" | "regex", caseSensitive}`. Secure values
are omitted. Large observations are private artifacts with bounded tool summaries.

Every node has an `elementId` and a canonical `role` such as `button`,
`text_field` or `text_area`, plus its platform role as `nativeRole`. Native role
names are also accepted in queries.

Frames are in points (`frameUnit`). The size of a screen in points depends on
the phone's Display Zoom setting as well as its model, so `ui_snapshot` and
`ui_find` also return `display`, the rectangle that point input divides by,
in the same unit. Each `ui_find` match whose frame center is on that display
also has `center`, the `{x, y}` fraction that `tap` takes. A physical iPhone
reports `display` from the runner installed by `connect` with `setup: true`;
an older runner reports none, and its matches have no `center`.

Lists, tables and collections build only the rows near the visible area, so an
item further down is not in the tree until the list is swiped. A `ui_find` with
no match on a screen holding such a list (role `scroll_view`) adds a `hint`
saying so; swipe the list and search again.

`ui_read` returns one fresh element found by `{query}` or by
`{snapshotId, elementId}` from an earlier read; a reference is freshly
revalidated, and changed trees refuse stale references. Reads report quality and
coverage; ambiguous queries return bounded choices, and truncation cannot prove
uniqueness. Narrow the query and read again instead of choosing the first result.

`wait` ends when any of 1..8 `conditions` holds, without sending input.
Conditions are `exists`/`gone` with `query`, `value` with `query` and a text
matcher, `state` with `query`, `state` and boolean `equals`, or `foreground` with
`appId`. Simulator states are `enabled` and `focused` where exposed; other
predicates fail before waiting and missing state is unknown. `timeoutMs: 0`
checks once; waits default to 10000 ms, allow up to 30000 ms and return last
evidence on timeout. Gone cannot be established from the simulator's partial
tree. Waits do not hold the input queue or repeat actions.

`ui_read` and `wait` support simulators and Android; a physical iPhone refuses
them with `CAPABILITY_UNAVAILABLE`. Physical accessibility reads through
`ui_snapshot` and `ui_find` require a ready runner, even on HID. Physical
inspection captures one native hierarchy snapshot. It reports enabled and
selected states; hittability is not supplied by that read and must not be
inferred. Explicit element input still checks current hittability before acting.

`clipboard_read` returns the text on the connected simulator's clipboard.
Physical iPhones refuse it.

## Apps

### Launch, terminate and restart

`app_launch`, `app_terminate` and `app_restart` are `act` actions that take an
`appId`. `app_restart` terminates the app, then launches it. Launch completion
only means the platform started the app; add `readiness` to `app_launch` or
`app_restart` to wait until its screen is shown:

```sh
testrun act '{"action":"app_restart","appId":"com.example.acme","readiness":{"element":{"identifier":"acme.home"},"timeoutMs":10000}}'
```

`readiness: {}` waits until the simulator's frontmost app is the launched process
and shows accessibility elements. `element` also waits for a matching element,
using the `ui_find` query fields (`identifier`, `role`, `label`, `value`).
`timeoutMs` is 100..30000 (default 10000). The result reports each stage —
`terminate` (with `wasRunning`), `launch` and `readiness` (`ready`, `not_ready`
or `not_requested`, with `frontmostIsLaunched` and `rendered`) — separately.

If a stage fails, the action fails with that stage's error code and its details
list every stage; completed stages are kept and nothing is retried. Unmet readiness
fails with `APP_NOT_READY` after the app launched. Inspect the screen before
deciding to restart again. Reset is not available, and restart never uninstalls
the app or erases the simulator.

On a physical iPhone, `app_launch` and `app_restart` use Apple's `devicectl` and
need no XCTest runner; they work for TestFlight and App Store apps too. Restart is
one launch that terminates any running copy first, so its `terminate` and `launch`
stages succeed or fail together. Physical launches cannot observe readiness:
requests with `readiness` are refused, and you inspect the returned image instead.
Launch arguments reach the app after a leading `--` argument, which keeps
`devicectl` from reading them as its own options. Launch uses CoreDevice even
when an XCTest runner is connected, so it does not wait for XCTest's animation
idle detection. `app_terminate` also uses CoreDevice without XCTest: it resolves
the installed app's executable path, requires a unique matching process,
rechecks identity before terminating and verifies absence afterwards. An already
stopped installed app succeeds without sending terminate.

### List installed apps

`app_list` (CLI `app list`) returns `appId`, `name`, `version` and `build`,
with a bounded summary and a full private JSON artifact. `name` takes a text
matcher, such as `{"text": "acme", "mode": "substring"}` (CLI `--name acme`),
and lists only apps whose name or app ID matches. Physical inventory uses
Apple tooling without XCTest. When Apple tooling returns no apps at all for a
physical device, not even system apps, `app_list` fails with
`CAPABILITY_UNAVAILABLE` instead of reporting an empty list.

### Install, update and uninstall

Use `app_install({targetId: "device:UDID", path: "/absolute/Staging.app"})`
to install a local signed build. Simulator targets accept Simulator `.app` builds
instead. The path belongs to the host running Testrun. To update an installed
app, add `replace: true`; Testrun never uninstalls first to work around signing
or version conflicts. Installation does not launch the app.

`app_install` and `app_uninstall` use the connected device unless `targetId` is
given. With a `targetId` that is not connected they run once on that device,
through its request queue, without connecting it, so they work before
`connect`; `status` still reports the result. On a connected device they wait
behind the requests ahead of them and leave the connection open.
`app_uninstall({appId: "com.example.staging"})` invokes normal platform
uninstall, including its local-data behavior.

```sh
testrun app install --target device:UDID /absolute/Staging.app
testrun app install --target device:UDID --replace /absolute/Staging.app
testrun app uninstall --target device:UDID com.example.staging
```

Each result has an `actionId`. After interruption, inspect it through
`status({actionId: "app-REFERENCE"})` or `testrun status --action-id
app-REFERENCE`. Status preserves uncertain outcomes and reads current installed
state when identity is known; it does not retry the mutation. `buildVerified`
indicates whether inventory actually exposed and matched the installed build.
Real deployment, data retention and developer-support/media coexistence still
depend on the device and build under test.

## Capture

### Screenshots

Simulator screenshots read the guest framebuffer directly. The simulator path
requires no guest helper or macOS window-sharing permission. Fast simulator
screenshots are compact JPEGs read from the current framebuffer; they do not
require opening a preview. Full evidence is preserved separately from the
bounded image attachment.

Screenshot results identify their `profile`, source and output dimensions, and
XCTest fast captures include acquisition/encoding timings. Video observations
report `captureTimingsMs` and frame PTS, while HID input reports dispatch timings.
Fast JPEG attachments use the compact artifact directly. XCTest input results split
coordinate construction from native execution. Lower-quality capture reduces
observation cost; it does not shorten XCTest gesture execution.

Simulator screenshots, previews and recordings share the connection's
framebuffer. Opening both preview formats or taking a screenshot during
recording does not require another capture connection. Stopping a preview
leaves recording and other previews running. A simulator restart invalidates
the connection; reconnect before sending another action, and inspect any
uncertain action rather than repeating it.

### Live preview

`preview_start` supports simulator H.264/JPEG and physical-device HEVC.
Omitting `format` selects H.264 for simulators and HEVC for devices; physical
H.264/JPEG profiles return `CAPTURE_REQUIREMENT_UNMET`. It returns a `previewId`,
a loopback WebSocket URL and a private Unix grant-broker path. A native
integration obtains a single-use, 30-second read-only token from that broker and
sends it as the first WebSocket message. Raw browser Origin requests are
refused. The preview sends versioned SBVF packets and readiness/statistics/stall
events. HEVC uses complete, four-byte-length-prefixed access units and an `hvcC`
configuration packet; metadata supplies the codec string and geometry. Physical
viewers use a bounded 64-frame queue with a 1 MiB access-unit limit. Compressed
video resumes after gaps at a keyframe; missing keyframe recovery ends with an
explicit timeout. The consumer must probe its decoder support before displaying
HEVC. `preview_status` and `preview_stop` take the `previewId`.

### Recordings

`recording_start` creates an owned MP4 recording with an optional
`maxDurationMs` up to ten minutes. `recording_status` and `recording_stop` take
its `recordingId`; a finished recording reports `status: "complete"`.
Finalization verifies a decodable video track, registers a private artifact, and
writes its manifest. Simulator publication failures keep the saved video's
artifact reference when the manifest cannot be written.

Physical recordings preserve HEVC without host re-encoding, decode every
finalized frame for verification, and segment configuration/geometry changes.
Recordings also split at keyframes after two seconds to bound full-track
verification work; failure to obtain a segment keyframe within five seconds of
source timestamps ends with verified partial evidence and `KEYFRAME_TIMEOUT`.
Each segment has its own verified artifact, ordered in the recording manifest.
Preview and recording share physical acquisition and can stop independently. A
recording reports startup frames skipped before its first keyframe separately
from recording drops. Acquisition stops when its last consumer ends, including
recording duration expiry without a follow-up status or stop call.

Physical preview and recording require iOS 27+, USB pairing and compatible
developer support exposing the CoreDevice display service. They need no runner
installation or signing. Mac Camera and Screen Recording grants are not requested
by this capture adapter; it starts from the Mac without a phone broadcast action.

## Physical iPhone details and troubleshooting

### HID video and gestures

HID observations decode continuously through VideoToolbox and return a frame
newer than the observation request. Startup calibrates physical display
dimensions using CoreDevice screenshot data and removes right/bottom codec padding
from video observations. Display references keep those physical dimensions.
A changed codec configuration/geometry refuses input with `STALE_GEOMETRY`;
disconnect and connect again for a freshly calibrated connection. Stale/ended
capture refuses input rather than using a cached frame. The controller keeps
acquisition even when a separately opened preview or recording stops.

HID gesture completion means `completion: "reports_sent"`, with
`deliveryAcknowledged: false` and `observed: false`. Verify the intended effect
from subsequent frames. Interrupted/failed sends quarantine the controller,
attempt release of its last contact, and keep an unknown outcome in its history.
Inspect observations/status, disconnect and connect again; do not replay the gesture.

### XCTest input

XCTest physical input validates the display geometry against a fresh runner image
before dispatch, without encoding or transferring that validation image. Gesture
results include runner `timingsMs` for geometry acquisition, scope resolution and
input; tool `elapsedMs` additionally includes host validation and USB RPC.

App-scoped single taps check foreground visibility, stable geometry and
observable covering system UI before submitting a complete touch event. A dialog
or transition returns `UI_OCCLUDED` or `UI_TRANSITION`; handle it explicitly and
observe the intended app before a new action. An incompatible XCTest runtime
returns `CAPABILITY_UNAVAILABLE`. Callback failure/timeout or a recorded XCTest
issue returns `OUTCOME_UNKNOWN` and blocks further runner input. Inspect status
and capture, then disconnect and connect again; never replay an uncertain tap.
Native completion alone does not prove the intended control changed.

Physical action results expose `rpcTimingsMs`: `roundTrip` measures USB
connection, request and complete response transfer; `runner` measures
authenticated runner handling through action/capture completion;
`hostAndTransport` is their nonnegative difference, including scheduling and
transport overhead. Screenshot `runnerTimingsMs.rpc` uses the same breakdown.
Unavailable runner measurements are null. These are durations on each host, with
no synchronized-clock assumption. Per-step `elapsedMs` also includes host
admission and validation; MCP's outer `elapsedMs` measures tool execution before
attachment encoding. Client-observed turnaround must be measured by the client
and includes delivery and model pacing.

The runner skips XCTest's implicit animation-idle waits. Focus, foreground,
visibility and geometry checks still apply; completion does not prove UI
readiness. An incompatible native idle-wait interface refuses runner setup with
`RUNNER_RUNTIME_UNSUPPORTED`.

### Runner on a HID connection

Operations that need the runner on a HID connection, such as accessibility reads
and evidence screenshots, return `RUNNER_SETUP_REQUIRED` without sending anything.
Call `connect` again with `setup: true`, choosing a team if needed; Testrun
replaces the connection without repeating input, and HID input keeps working
alongside the runner. Add the runner whenever HID is not enough: a small or
crowded target (find it with `ui_find` and tap the match's `center`),
checking a field's text or focus, or a full-quality screenshot. Elements an app
does not expose to accessibility still need the image.

App-scoped XCTest input returns `APP_BACKGROUNDED` before sending anything when
that app is not in the foreground. Use `{"kind":"system"}` for the Home Screen,
Spotlight, TestFlight or another app.

If accessibility inspection expires, `INSPECTION_TIMEOUT` identifies a failed
read. `status` reports `health.inspectionHealth` separately from HID/video
health. `RUNNER_BUSY` means the new request was refused without being queued;
heartbeat and fast HID observations can remain healthy while inspection finishes.
Once the read finishes, inspection can resume. If it remains busy, disconnect and
reconnect with the same runner setup options. Disconnect can terminate the owned
runner's blocked inspection; it never replays input. A new runner does not accept
old element references.

Runner health reports `activeAction` and `activeElapsedMs` while work is in
progress. Disconnect refuses new runner work at once. An input still running
gets up to 15 seconds from its start to finish. Past that, XCTest is stuck and
cannot cancel it, so the runner exits on its own. That input's outcome stays
unknown and is never repeated. After an unknown outcome, `act` replaces the
connection on its own. If cleanup failed and you did not disconnect, the next
`act` or `observe` retries it and reconnects, so no manual release is needed.
If the runner has exited, cleanup verifies its recorded PID is absent from the
selected phone before releasing the runner. An unreadable process inventory
keeps cleanup unfinished. Exited XCTest hosts report lost
XCTest control; an independent HID connection can keep working. Later runner
operations return `RUNNER_EXITED`. An input or app launch/terminate sent as the
host exits returns `OUTCOME_UNKNOWN`; it is not repeated, so observe the screen,
then disconnect and connect again with setup.

### Runner setup failures

If the phone already has a Testrun runner signed by another team, iOS refuses to
replace it in place; Testrun then uninstalls its own runner app and installs the
new one. It removes no other app. A failed preparation returns `RUNNER_BUILD_FAILED`,
`RUNNER_INSTALL_FAILED` or `RUNNER_CONFIGURE_FAILED`; its details name the `step`,
include a short `diagnostic` from the Xcode or `devicectl` output, and give the
`log` path of the full private output, which is kept for failed preparations.
A runner that installs but never answers reports its XCTest host output the same way.
An untrusted developer refusal returns `UNTRUSTED_DEVELOPER`: on the phone,
open Settings > General > VPN & Device Management > Developer App and trust the
runner's developer, then run setup again. Other host exits during startup report
`RUNNER_START_FAILED` and the diagnostic. Profile checks distinguish
`RUNNER_PROFILE_INVALID`, `RUNNER_PROFILE_EXPIRED` and `RUNNER_SIGNATURE_INVALID`.
A missing test-manager service is a startup failure, not evidence that the phone
is locked.

### Disconnect and capture conflicts

If `disconnect` reports `CLEANUP_FAILED`, address the prerequisite in
`details.nextAction`, such as unlocking or reattaching the phone. Testrun keeps
the phone and retries the release on its own; `status --target` shows its
progress, and `testrun manager retry --target device:UDID` runs it at once.
Completed release stages and input are not repeated. Status reads do not
perform the release; prior release errors remain available after it succeeds.
See [release, idle timeout and recovery](#release-idle-timeout-and-recovery).

`TARGET_BUSY` and local `CAPTURE_CONFLICT` include a private lock path and advisory
owner PID. Confirm the holder with `lsof`; disconnect it or allow cleanup to
finish. Persistent `.lock` files are normal and must not be deleted to recover.
An external CoreDevice conflict instead reports `blockingState: "remote_media_session"`
and session count. Close that capture through the tool that opened it. After a
capture process exits abruptly, feedback expiry takes about 5 seconds for a Testrun stream and can
take longer for other tools' streams; wait and retry.
`retryAfterMs` is guidance, not proof the source will be free. Testrun never stops
foreign capture or terminates Xcode. Discovery reporting a connected phone does
not imply its media source is available.

Last-subscriber stop closes its transport, sends participant-specific RTCP BYE,
and verifies expiration of its own remote session. Allow up to 30 seconds for
remote cleanup, including the device's feedback timeout, which Testrun sets to
5 seconds for its video stream.
Other media clients are preserved.

## Commands with an explicit target

With `--target`, commands use the named device. Some
[reads connect the device automatically](cli.md#automatic-connections-for-reads).
Input and network changes require an existing connection. `app install` and
`app uninstall` run once on a named device without connecting it, as described in
[several devices and named targets](#several-devices-and-named-targets).
Connect once, then name the target on each command:

```sh
testrun connect --target simulator:SELECTED-UUID
testrun observe --target simulator:SELECTED-UUID --profile evidence
testrun ui snapshot --target simulator:SELECTED-UUID --allow-partial
testrun ui find --target simulator:SELECTED-UUID --role button --label Continue --allow-partial
testrun app list --target simulator:SELECTED-UUID
testrun act --target simulator:SELECTED-UUID '{"action":"tap","point":{"x":0.5,"y":0.3}}'
testrun act --target simulator:SELECTED-UUID '{"action":"open_url","url":"https://example.com/acme"}'
testrun disconnect --target simulator:SELECTED-UUID
```

## Retained socket transport

`connect` handles connection lookup and release through the background manager.
Hidden CLI commands instead run the underlying controller in a foreground
process of its own and expose it over a local socket, for developers
integrating it directly. They work for iOS and Android targets.

```sh
testrun session --target device:SELECTED-UDID --control-path hid --directory /tmp/acme-qa-session
testrun session --target device:SELECTED-UDID --control-path xctest --app-id com.example.acme --setup \
  --runner-team-id TEAM_ID --directory /tmp/acme-inspection-session
```

`session` runs in the foreground and owns authorization, action journals,
captures, the optional runner and cleanup. The directory must be new. Startup
returns its private `control.sock` endpoint, selected `controlPath` and an
initial observation. `--app-id` selects app scope and requires
`--control-path xctest`; `--setup` permits owned runner preparation. Simulator
sessions can omit setup and app scope. `--observation-profile fast|evidence`
sets the default observation; HID defaults to `fast`, other sessions to
`evidence`. Idle sessions close after five minutes; `--idle-timeout-ms` allows
1..600 seconds. Endpoints accept only local clients with the same user identity.

```sh
testrun batch --endpoint /tmp/acme-qa-session/control.sock --command-id move-1 \
  '[{"tool":"tap","arguments":{"point":{"x":0.3,"y":0.4}}},{"tool":"swipe","arguments":{"from":{"x":0.3,"y":0.4},"to":{"x":0.6,"y":0.4},"durationMs":100}}]'
testrun status --endpoint /tmp/acme-qa-session/control.sock --command-id move-1
testrun close --endpoint /tmp/acme-qa-session/control.sock
```

A batch accepts 1..8 ordered steps. It validates all steps before acting, stops
on the first failure, and returns per-step timings plus a fresh final screenshot.
A final screenshot step is reused as the observation; `--no-observe` skips the
implicit capture, and `--observe-after-each` returns each step's screenshot as
`steps[i].observation`. Repeated command IDs return the kept result; changed
payloads conflict. Unknown outcomes stop further input and require
status/observation and a new controller, rather than replay.

Each session keeps a private `outcomes/` journal in its directory. After
`BATCH_OUTCOME_UNKNOWN`, use the same `status --endpoint ... --command-id ...`
command even if the socket has disappeared. Status reads the archive when the
`session` process is unavailable and marks it `journalSource: "archive"`. Batch status
includes `actionOutcomes` for separately recorded mutations. A dispatch without a
recorded terminal result stays `outcome_unknown`; a missing record provides no
basis for replay. Archives cannot reacquire control or execute input. Keep the
session directory until investigation is complete.

During orderly shutdown, the endpoint stays available for `status` and `close`
while resources finalize; it reports `state: "closing"`, `closed: false`. New
batches are refused with `SESSION_CLOSING`. Removing or replacing the owned
socket triggers cleanup within one second; cleanup never removes a replacement
socket belonging to another process. Ctrl-C, SIGTERM and idle expiry start
best-effort shutdown. If cleanup fails, the process exits with `CLEANUP_FAILED`
after attempting the remaining cleanup. Process exit does not prove device-side
release or authorize replay of input.
