# Changelog

Each release's entry becomes its notes on GitHub and on
https://canonic.sh/workbench/changelog/. The release workflow refuses a
`workbench/v<version>` tag whose version has no entry here.

Write an entry as plain paragraphs and `-` list items: the website shows those
and leaves out headings.

## 0.9.0

- Screenshots and handoffs of Storybook and URL lenses show the page as you see it: an opened modal or menu, typed text, and scroll positions. Workbench loads those pages through a local proxy that adds its preview bridge, so your Storybook and app need no changes. If you added the bridge loader to `.storybook/preview` yourself, you can remove it. See Storybook and Lenses and URL implementations.
- Handoffs through a lens name the element under each mark as it is on the page you see.
- Lens pages load even when the app sends `X-Frame-Options` or a `frame-ancestors` policy.
- Sign-in through an identity provider (OAuth or SSO) doesn't stay signed in inside a lens, because the provider returns to your app's own address rather than the proxy. Use a sign-in your development build serves itself. See Embedding and sign-in in Lenses and URL implementations.

## 0.8.0

- Screenshots and handoffs come only from the capture helper bundled with the extension. Workbench no longer falls back to Chrome, and the `canonic.capture.chromePath` setting is gone; you can remove it from your settings.
- On a host that can't run the helper (macOS 12 or earlier, or Linux without a display), screenshots fail with that reason instead of opening Chrome. See The VS Code extension guide.
- New guides for previewing React, React Native Web, Vue, Astro, and HTML, and for registering custom adapters. The extension guide now covers which files Workbench writes, its network access, updating, and uninstalling.

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
