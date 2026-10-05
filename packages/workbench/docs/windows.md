# App windows

A `window` implementation streams a window of any running macOS app onto the
canvas: an Android emulator, a desktop app, a native build, or anything else
with a window. You can compare it with the design, annotate it, and hand it
off like any other page. The stream is for viewing; clicks on it don't reach the
app.

## Requirements

- **macOS 13 or later with Xcode, or the Xcode Command Line Tools**
  (`xcode-select --install`). Their Swift compiler builds the capture helper
  the first time a window is streamed.
- **Screen Recording permission** for your editor. See [Permissions](#permissions).
- The window must be **open and visible on the display**. A minimized window
  or one on another Space can't be captured.

## Configure it

Declare the app once, then name the window on each page that shows it:

```yaml
implementations:
  emulator:
    kind: window
    app: com.example.emulator

collections:
  - name: Mobile
    items:
      - label: Sign in
        src: design/sign-in.html
        sizes:
          - mobile
        implementations:
          emulator: Example Phone
```

The page gets an **Emulator** lens next to **Design**. Bring the app to what
the design shows, then switch between the two to compare.

`app` is the application's bundle ID, or any part of it: `com.example.emulator`
and `emulator` both match an app whose bundle ID is `com.example.emulator`.
To find an app's bundle ID, run `osascript -e 'id of app "Example"'`.

The value on the page is the window's title, or any part of it, ignoring
case. When several of the app's windows match, the largest one is streamed.

`root` and [code pointers](lenses.md#point-at-the-code) work as for other
implementations. `start` and `catalog` aren't available for windows.

## How it works

- A small native helper, built from source shipped in the extension, finds the
  window with ScreenCaptureKit and encodes it once with VideoToolbox. It's the
  same helper the [iOS Simulator](ios-simulator.md) lens uses, and one window
  streams at a time. Two canvases streaming at once, such as the editor tab
  and a browser opened with **Open Canvas in Browser**, interrupt each other.
- The server looks up the app and the title in `workbench.yaml`. The canvas
  can only ask for a window that a page declares.
- In VS Code, JPEG images reach the canvas over a loopback HTTP stream at about
  20 frames per second. A standalone browser uses an H.264 stream at about 30
  frames per second, decoded with WebCodecs.
- There is no fallback to browser screen sharing. If the helper can't capture,
  the canvas says why.

## Permissions

macOS attributes Screen Recording to the app that launched the helper, which is
your editor. The first time, macOS asks, or the canvas reports a denial and
opens the right settings pane. Then:

1. Open **System Settings** › **Privacy & Security** › **Screen & System Audio Recording**.
2. Turn on **Visual Studio Code**, or the editor named in the error.
3. Quit and reopen the editor.
4. Pick the page again.

When you run the server without the editor, grant the permission to the
terminal app that started it.

## Screenshots and exports

The camera and handoff capture the stream's current image with your
annotations. A [design-system export](design-system-export.md) captures the
design page, not the window.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| “No visible … window was found” | Open the window and keep it on the display. The error lists the visible windows: check that `app` is part of the bundle ID and the page's value is part of the title. |
| Black canvas or a permission error | Grant Screen Recording to the editor, then restart it. |
| The wrong window shows | Make the page's value more specific. The largest matching window wins. |
| The lens is missing | Read the [config route](troubleshooting.md#read-the-resolved-config)'s `problems`: `app` must be a bundle ID or part of one, with no spaces. |
