# Investigate Android crashes, ANRs and disappearance

Testrun collects bounded Android evidence through an already installed `adb`.
Select an authorized target, exact application package and Android user. Android
26+ is required; historical exit reasons require Android 30+. This works without
a debuggable build when the OS grants the individual reads. Physical devices,
emulators and OEM builds still have different access restrictions.
For resource measurements without crash-log collection, use the separate
[`android_state` source](diagnostic-android-state.md).

```sh
testrun diagnostics capabilities
testrun diagnostics open --submission-id acme-crash --question "Why did Acme disappear?" --binding '{"targetId":"android:SELECTED-SERIAL","appId":"com.example.acme"}'
testrun diagnostics start diag-RETURNED_REFERENCE --submission-id android-window --source '{"kind":"android","targetId":"android:SELECTED-SERIAL","appId":"com.example.acme","userId":0,"mode":"snapshot"}'
testrun diagnostics status diag-RETURNED_REFERENCE
```

The investigation's target/app binding must match the source. `userId` is the
Android user/profile number, not a Linux UID. `snapshot` reads one bounded
sequence; `follow` repeats process/log/exit checks until stopped or bounded.
Use `follow` around an independently authorized reproduction. Collection does not
launch or relaunch the app. Reuse submission IDs after a lost reply.

Optional source fields are `systemContext` (default true), `includeReports`
(default false), and `since` (device epoch seconds with 1–9 decimal places).
`includeReports` attempts at most four safe report paths actually referenced by
the selected UID's exit history. Denied or missing reports remain gaps. It never
enumerates private report directories or enables root. No bugreport is generated.

## Read the result

Poll status until `settled: true`. A completed sampling sequence has collector
phase `partial` and reason `android_sampled_window`: circular logs, old process
lifetimes and OS history retention prevent a claim of complete observation.
`sourceReport` contains API level, observed UID/process generations, cycles and
loss codes, without captured messages. Parse the returned collector:

```sh
testrun diagnostics analyze diag-RETURNED_REFERENCE collector-RETURNED_REFERENCE --submission-id parse-android
testrun diagnostics analysis-status diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"failure"}' --limit 100
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"process"}'
testrun diagnostics query diag-RETURNED_REFERENCE analysis-RETURNED_REFERENCE --filters '{"kind":"stack"}'
```

Follow pagination. Each row includes a raw evidence digest and source witness.
Java exceptions, native signals, ANRs, low-memory/system kills, watchdog,
background-policy and package-replacement messages are classified candidates.
Historical OS exit reasons preserve unknown reason codes. Competing candidates
remain separate, with `causeEstablished: false`; proximity is not causal proof.
Zero matching rows never proves that no failure occurred.

Installed evidence records observed version, split paths, debuggable/profileable
flags, processes and service records. Unknown flags stay null. Available native
libraries have bounded `readelf` identity/section inspection when that command
exists on the target; raw frames stay unresolved. No local checkout, downloaded
symbols or guessed matching build supplies missing facts.

## Scope and collection limits

ADB commands pin the observed transport ID and boot ID. Process generations use
boot ID, PID and `/proc` start ticks, with real UID validation; PID equality alone
does not join generations. Already historical log rows keep unknown generation.
A reboot or package UID/version/update change interrupts collection. A process
missing from a later snapshot is `noLongerObserved`, without proving its death.
Shared UIDs require named-process association; isolated child UIDs and inaccessible
`/proc` state can leave gaps.

App logs request only the installed UID from `main`/`crash`. Optional system
context samples `system`/`events`/`crash`, retaining privileged-writer messages
with the exact package and user/UID. A native DEBUG backtrace additionally needs
a package and UID header in the same bounded block. Cross-user, neighbor and
unattributed blocks are excluded. System passes omit duplicate app-UID rows.
Package/service dumps and referenced reports can contain related processes/users;
raw evidence remains private. Filtering does not make arbitrary logs secret-free.

Each log snapshot is at most 512 KiB, initially the last 512 lines. Subsequent
reads use inclusive epoch cursors and boundary multiplicity to preserve repeated
identical entries. Regressions, unparsed lines, command failure and output limits
are explicit gaps. Ring overwrites between polls remain unknown. The collector
never clears log buffers, resets the app, installs helpers, attaches a debugger,
changes settings or uploads evidence. ADB itself may start its host server.

Shared limits default to 30 seconds/1 MiB/4096 chunks, with maxima of
300 seconds/8 MiB/65536 chunks. Additional caps are 256 cycles, 32 processes,
8 native libraries and 4 referenced reports. Follow cycles wait at least 500 ms
plus command time. Each command has at most four seconds, further bounded by the
remaining collection duration. Stop acknowledges promptly; settlement waits for
the current bounded client to finish or be killed/reaped. It preserves app and
ADB server state. Target release and manager drain stop their owned readers.

Use `diagnostics read` only for explicit private raw bytes. Imported UTF-8
`android-tombstone`, `android-anr` and `android-bugreport-text` files can be
analyzed offline; broad bugreports require deliberate file selection and remain
broad, with unknown per-row app scope. Binary protobuf/ZIP reports and full native
symbolication are not decoded. See [formats](diagnostic-formats.md) and
[privacy/retention](diagnostic-access.md). Native app/capture overhead and live
physical/emulator/OEM behavior have not been measured by the owned fixture tests.
