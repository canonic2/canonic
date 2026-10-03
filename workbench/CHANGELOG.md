# Changelog

Each release's entry becomes its notes on GitHub and on
https://canonic.sh/workbench/changelog/. The release workflow refuses a
`workbench/v<version>` tag whose version has no entry here.

Write an entry as plain paragraphs and `-` list items: the website shows those
and leaves out headings.

## 0.7.0

- Define screens and components in `.workbench.ts` or `.workbench.tsx` files, with named states and adapters for HTML, React, Vue, Astro, and React Native Web. See the TypeScript Workbench previews guide.
- Use **Preview controls** to edit inputs, reset a state, inspect actions, and read documentation. Edits stay temporary.
- Compare a design with a TypeScript preview using a `workbench` lens.
- Export compiled previews in the design-system ZIP, or build a standalone browser viewer with the included command-line tool. The viewer includes states, viewports, controls, actions, and documentation; Astro exports include authored states without editable inputs.
- The canvas opens with a loading indicator while services start, and the sidebar identifies pending catalogs and preview discovery.
- Screen switching keeps the current screen visible while the next loads, reuses managed preview renderers, and avoids waiting for offscreen lazy images.

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
