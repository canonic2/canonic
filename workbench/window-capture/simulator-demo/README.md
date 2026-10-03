# iOS Simulator stream harness

This harness exercises the window capture helper in the parent folder
(`window-capture/Capture.swift`), the same ScreenCaptureKit and VideoToolbox
helper the Canonic workbench uses, pointed at the Simulator with
`--app simulator`. It relays low-latency H.264 over a local WebSocket and
decodes it into a browser canvas with WebCodecs, without the rest of the
workbench UI.

Requirements: macOS 13 or newer, Xcode, a booted and visible iOS Simulator,
and a Chromium browser with WebCodecs. The first run may request Screen
Recording permission for the application running the demo (usually Terminal);
restart that application after granting permission.

```sh
cd packages/workbench/window-capture/simulator-demo
node server.js
```

Open `http://127.0.0.1:4587/`. If several Simulator windows are visible, pass
part of the desired window title, for example `node server.js "iPhone 17"`.
Set `CANONIC_SIMULATOR_STREAM_PORT` to use another port.

If macOS has not granted Screen Recording permission, enable the application
running the demo (usually Terminal) in Screen & System Audio Recording and
restart it. The workbench opens that settings pane automatically after a denial.

Click **Enable interaction** to start a persistent WebDriverAgent session for
the booted Simulator. Once enabled, clicks on the captured device become WDA
taps and pointer drags become WDA swipes. The mapper excludes the Simulator
window toolbar by fitting the WDA-reported device screen to the bottom of the
captured window. The visual debugger reports WDA startup and gesture latency.

Install WDA once before using interaction:

```sh
./ios install-wda
```

The workbench adds lifecycle, WDA input, screenshots, handoffs, and the
`ios-simulator` manifest kind around the same capture core.
