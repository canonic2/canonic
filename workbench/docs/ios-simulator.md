# iOS Simulator

An `ios-simulator` implementation streams a booted iOS Simulator onto the
canvas. You can tap and drag on it, annotate it, and hand it off like any
other page. It is useful for comparing a native app with its design, or for
reviewing a native app alongside web pages.

## Requirements

- **macOS 13 or later with Xcode**, which builds the capture helper the first
  time a Simulator is streamed, and at least one booted Simulator.
- **Screen Recording permission** for your editor. See [Permissions](#permissions).
- **WebDriverAgent (WDA)** for taps and drags, installed through
  [Canonic Shield](#install-webdriveragent)'s iOS automation in the project.
  Without it the stream still shows, but input doesn't reach the device.

## Configure it

### Import booted devices

```yaml
implementations:
  simulator:
    kind: ios-simulator
    device: booted
    catalog: true
```

With `catalog: true`, each matching booted device becomes a page in a
collection named after the implementation. These pages have no design. They
open straight on the stream.

`device` chooses which Simulators match:

| Value | Matches |
| --- | --- |
| `booted` (default) | Every booted Simulator |
| A device name, such as `iPhone 16 Pro` | That device, when booted |
| A UDID | That exact device |

A Simulator config can be the whole `workbench.yaml`, because a catalog
supplies pages.

### Add a Simulator lens to a design page

A design page can also show a booted device in a lens. The lens streams only
a device that the implementation's catalog has imported, so keep
`catalog: true` on the implementation and name the device by its UDID:

```yaml
implementations:
  simulator:
    kind: ios-simulator
    catalog: true

collections:
  - name: Mobile
    items:
      - label: Onboarding
        src: design/onboarding.html
        viewports:
          - mobile
        implementations:
          simulator: 6A1F2B3C-0000-4000-8000-123456789ABC
```

To find a booted device's UDID, run `xcrun simctl list devices booted`. A
device name in this place is accepted by the reader, but the stream refuses
it, and the canvas reports that the Simulator isn't declared.

The page gets a **Simulator** lens next to **Design**. Bring the app on the
device to what the design shows, then switch between the two to compare.

`root` and [code pointers](lenses.md#point-at-the-code) work as for other
implementations. `start` isn't available for the Simulator.

## How it works

- A small native helper, built from source shipped in the extension, finds the
  Simulator's window with ScreenCaptureKit and encodes it once with VideoToolbox.
  It's the same helper the [app window](windows.md) lens uses, and one window
  streams at a time. Two canvases streaming at once, such as the editor tab
  and a browser opened with **Open Canvas in Browser**, interrupt each other.
- In VS Code, JPEG images reach the canvas over a loopback HTTP stream at about
  20 frames per second, which doesn't depend on the editor's media codecs. A
  standalone browser uses an H.264 stream at about 30 frames per second,
  decoded with WebCodecs.
- Taps and drags on the canvas are sent to the device through WebDriverAgent.
- There is no fallback to browser screen sharing. If the helper can't capture,
  the canvas says why.

## Permissions

macOS attributes Screen Recording to the app that launched the helper, which is
your editor. The first time, macOS asks, or the canvas reports a denial and
opens the right settings pane. Then:

1. Open **System Settings** › **Privacy & Security** › **Screen & System Audio Recording**.
2. Turn on **Visual Studio Code**, or the editor named in the error.
3. Quit and reopen the editor.
4. Pick the Simulator page again.

When you run the server without the editor, grant the permission to the
terminal app that started it.

## Install WebDriverAgent

Workbench sends input through the iOS automation in
[Canonic Shield](https://github.com/canonic2/canonic). With Shield installed in
the project, install WebDriverAgent once from the project root:

```sh
node .canonic/src/automation/ios-cli.mjs install-wda
```

This clones WebDriverAgent into `.canonic/.dependencies/ios`. Run it again
with `--force` to replace an existing copy. The first input to a device starts
a WDA session, so expect a delay before the first tap lands.

## Screenshots and exports

The camera and handoff capture the stream's current image with your
annotations. A [design-system export](design-system-export.md) doesn't
capture Simulator-only pages; each one is listed under `captureWarnings`
instead of getting a reference image.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| No Simulator pages | Boot a Simulator. With a `device` name or UDID, check that it matches exactly. The config route's `problems` reports when no booted device matches. |
| Black canvas or a permission error | Grant Screen Recording to the editor, then restart it. |
| A design page's Simulator lens says the Simulator isn't declared | Name the device by its UDID, keep `catalog: true` on the implementation, and boot the device. See [Add a Simulator lens to a design page](#add-a-simulator-lens-to-a-design-page). |
| Stream shows but taps do nothing | Install WebDriverAgent. Check the **Workbench** log (**Workbench: Show Log**) for automation errors. |
