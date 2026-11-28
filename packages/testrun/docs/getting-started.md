# Getting started

Choose the host where Testrun runs and the target you want to test. Then
capture a mobile screenshot or start a browser and verify its page list.

## Before you begin

Run `testrun --help` to check that the command is available. See
[Distribution](distribution.md) for release and installation availability.

## Choose your host and target

| Testrun host | iOS | Android | Managed browsers |
| --- | --- | --- | --- |
| macOS | [iOS simulator](#prepare-an-ios-simulator) or [physical iPhone](ios.md#physical-iphone-readiness) | [Android emulator](#prepare-an-android-emulator) or [Android device](android.md#readiness-diagnosis) | [Try a browser](#try-a-browser) on Intel macOS 15 |
| Windows | Use a macOS host for iOS | [Android emulator](#prepare-an-android-emulator) or [Android device](android.md#readiness-diagnosis) | [Try a browser](#try-a-browser) with a Windows x64 build |
| GNU/Linux | Use a macOS host for iOS | [Android emulator](#prepare-an-android-emulator) or [Android device](android.md#readiness-diagnosis) | Use a supported macOS or Windows x64 host |

### Prepare an iOS simulator

Install Xcode and the selected simulator runtime, then boot a simulator you
intend to test. See [iOS setup](ios.md#setup-and-readiness) for the host checks.
Continue to [Find your target](#1-find-your-target).

### Prepare an Android emulator

Install Android SDK platform tools and put `adb` on `PATH`. Boot an AVD you
intend to test and wait for Android to finish starting. See
[Android setup](android.md#setup-and-readiness) for readiness and emulator options.
Continue to [Find your target](#1-find-your-target).

## 1. Find your target

Run this in a terminal on the host where your target is running:

```sh
testrun target list --compact
```

Copy the full ID of the running target. It looks like `simulator:…` for an iOS
simulator or `android:emulator-5580` for an Android emulator. An
`android:avd:NAME` entry refers to the saved emulator configuration. Choose
the running emulator's ID for this walkthrough. If you only see the AVD or
the device is missing, follow
[device troubleshooting](troubleshooting.md#the-device-is-missing-or-unavailable).

## 2. Connect and capture the screen

Replace `TARGET_ID` in each command with the ID you copied:

```sh
testrun connect --target TARGET_ID
testrun observe --target TARGET_ID --profile evidence
```

Wait for `connect` to report that the target is ready before running `observe`.
The observation returns the capture dimensions and a screenshot. Open the path
reported in the result and check that it shows the target's current screen.
If observation fails after connecting, compare the reported error with the
[iOS capture guide](ios.md#capture) or [Android capture guide](android.md#capture).

## 3. Release the target

```sh
testrun disconnect --target TARGET_ID
```

This releases Testrun's connection. An emulator or simulator you started
yourself stays running.

## Try a browser

### Check the host

On a supported [browser host](distribution.md#host-requirements), check Chrome:

```sh
testrun browser doctor --engine chrome
```

If doctor reports `hostSupported: true`, start a headless Chrome session:

```sh
testrun browser connect --engine chrome --mode headless
```

### Wait for startup

The first connection prepares a private browser and may download its pinned archive.
Copy the returned `browser:` target ID in place of `BROWSER_TARGET` and keep it
for the remaining commands:

```sh
testrun browser status --target BROWSER_TARGET
```

The connect reply confirms that preparation was submitted. Check the same
target until `browser.phase` is `ready`. If startup fails, read the failure and
follow [browser troubleshooting](troubleshooting.md#the-browser-does-not-start).

### Confirm the page list

Request a page list. On Windows, follow the CLI guide for
[passing JSON arguments](cli.md#pass-json-arguments).

```sh
testrun browser send --target BROWSER_TARGET '{"operation":"page_list"}'
testrun browser status --target BROWSER_TARGET --operation-id OPERATION_ID --wait-ms 30000
```

Replace `OPERATION_ID` with the ID returned by `browser send`.
The page list is available when `operation.settled` is `true` and
`operation.outcome` is `succeeded`. If the wait expires with `settled: false`,
poll the same ID so you keep following the submitted request. See
[interrupted operations](troubleshooting.md#an-action-timed-out-or-cleanup-is-pending)
for other outcomes.

### Close the browser

Disconnect the same browser target:

```sh
testrun browser disconnect --target BROWSER_TARGET
```

## Next steps

- [Read or act on the screen](ios.md#connect-and-act) on iOS, or follow the
  [Android workflow](android.md#connect-and-act).
- [Connect a coding agent](mcp.md#agent-setup) to run the same workflow through MCP.
- [Read and control a browser page](browser.md#read-and-control-a-page) using the returned page reference.
- [Collect diagnostic evidence](debugging.md) when you need to investigate an app failure.
