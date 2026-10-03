# Changelog

Each release's entry becomes its notes on GitHub and on
https://canonic.sh/workbench/changelog/. The release workflow refuses a
`workbench/v<version>` tag whose version has no entry here.

Write an entry as plain paragraphs and `-` list items: the website shows those
and leaves out headings.

## 0.6.0

- New `window` implementation kind: stream a window of any running macOS app onto the canvas, such as an Android emulator or a desktop build. Declare the app's bundle ID once, and map each screen to part of a window title. See the App windows guide.
- The window stream is view-only and shares one native stream with the iOS Simulator lens. Screenshots and handoffs capture its current frame.
- Fixed iOS Simulator streaming in installed builds: the extension now ships the source of its native capture helper, which earlier packages left out.
- The capture helper is now named Canonic Window Capture. The first stream after updating rebuilds it once.

## 0.5.1

- Workbench's user guides are published at https://canonic.sh/workbench/docs/, alongside new Install and Changelog pages. The extension README links to them.
- No changes to the extension's behavior.

## 0.5.0

- The extension is now called Workbench, with a new icon.
- The canvas zooms and pans, with Figma's shortcuts, a mouse wheel, or a trackpad pinch.
- Redesigned sidebar navigation and canvas toolbar.
