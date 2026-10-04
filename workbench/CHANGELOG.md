# Changelog

Each release's entry becomes its notes on GitHub and on
https://canonic.sh/workbench/changelog/. The release workflow refuses a
`workbench/v<version>` tag whose version has no entry here.

Write an entry as plain paragraphs and `-` list items: the website shows those
and leaves out headings.

## 0.12.2

- Action needed: `workbench.yaml` uses new names. Rename `sections` to `collections`, `folder:` to `group:`, and `projects` to `spaces`, in `workbench.yaml` and in `workbench.local.yaml`. A file that still uses the old names shows an empty sidebar and reports no problem. See the workbench.yaml reference.
- Action needed: Workbench requires VS Code 1.123 or later, and running `server.js` from the command line requires Node 24 or later.
- Docs pages document a component or part of your design system in Markdown, with live examples rendered by your own components, each example's code, and props tables. A docs page fills the canvas on white and scrolls like a web page. Add one by giving a page a Markdown `src` and an `examples` implementation. See Docs pages.
- The interface uses the same words as the file. The switcher at the top of the sidebar changes spaces, the sidebar lists collections, groups, pages, and states, and what you draw on a page is an annotation. The commands are Workbench: Switch Space…, Workbench: Add Space…, and Workbench: Refresh Pages; update any keybindings for the old command IDs. Spaces you added from elsewhere on disk with Add a project… aren't carried over: add them again with Add a space…. See Several spaces and Annotations and handoff.
- Agents and scripts that read `/_workbench/config` or the handoff prompt get the same new words, such as `collections` and `pages`.
- **Recenter view**, beside the zoom buttons, brings the artboard back to the middle of the canvas without changing the zoom level or the page's scroll position. See Using the canvas.
- TypeScript previews compile faster: each platform's extension carries a native compiler, and compiled previews are kept on disk between sessions.
- A preview and a Storybook story whose titles name the same collection and group appear together in that group, with your `workbench.yaml` pages.
- SVG icon sprites a preview registers when it loads stay available when you switch states or come back to the preview.
- Workbench is also published on Open VSX, for editors that install extensions from there.
- Workbench is distributed under the Canonic Proprietary License instead of the MIT license. See `LICENSE` in the extension.

## 0.11.1

- Workbench is on the Visual Studio Marketplace as Canonic Workbench. Install it from the Extensions view in VS Code. Existing installs from a VSIX file are the same extension, so the Marketplace updates them from now on.
- The Extensions view lists the extension as Canonic Workbench. The activity bar, views, and commands still say Workbench. Nothing else changes.

## 0.11.0

- Preview definitions type-check in your project. Install the types with `npm install --save-dev @canonic2/workbench`, and `tsc` and your editor resolve `import { definePreview } from '@canonic2/workbench'`. Workbench still compiles previews with its own copy, so the installed version affects types only. See Types in TypeScript Workbench previews.
- Action needed: preview definitions and `workbench.config.ts` import from `@canonic2/workbench`. Change `from '@canonic/workbench'` to `from '@canonic2/workbench'` in your `.workbench.ts` and `.workbench.tsx` files; previews that still import the old name fail to build. If you use a `workbench-env.d.ts` from `init`, delete it and run `init` again, or install the package instead.
- Workbench is on the Visual Studio Marketplace: search for Workbench in the Extensions view, or update from there from now on. The VSIX files stay on GitHub Releases.

## 0.10.0

- Workbench previews behave like Storybook stories: they stay on the canvas. Map the addresses a preview links to in its definition's new `links`, and with Actions on those links open the mapped preview. Every other link and form, including routes without a preview, other sites, and downloads, is recorded under Actions instead of followed. `context.navigate` opens a preview from a script. See Links and navigation in Preview data, mocks, and actions.
- Links to a spot on the same page, such as `#features`, scroll to it whether Actions is on or off.
- Previews can answer the requests their page makes. A definition's new `requests` mocks `fetch` and XMLHttpRequest calls by method and path, or by GraphQL operation name, with per-state overrides for empty, loading, error, and offline states. It works with any adapter, and with `fetch` in Astro frontmatter. See Preview data, mocks, and actions.
- `environment` in `workbench.config.ts` wraps every preview in shared providers and setup, like Storybook's global decorators. Projects that mix frameworks can give each adapter its own.
- A new guide, Preview data, mocks, and actions, covers inputs and controls, fixtures, environments, request mocks, the action log, and links, with recipes for common kinds of screens.
- One window can switch between several projects, such as a product and its design system. A switcher at the top of the Workbench view lists every folder of the window that has a `workbench.yaml`, plus projects you add from elsewhere on disk with Add a project…, which every window remembers. With more than one project, the canvas toolbar starts with the project too. Each project keeps its own server, port, and settings, and starts the first time you open it. A window with one project works as before. See Several projects.
- One `workbench.yaml` can describe several projects under `projects`, each with its own name, mark, screens, and implementations, sharing the file's folder or serving its own `root`. Keys outside `projects` are shared by all of them. See Projects in the workbench.yaml reference.
- A multi-root workspace lists the projects of every folder with a `workbench.yaml`, instead of serving only the first folder.
- New commands: Workbench: Switch Project… and Workbench: Add Project….
- `color` and `icon` at the top of `workbench.yaml` mark the project in the switcher, beside its `name`. A color is a named color or a hex value; an icon is a Lucide icon name or an image in the project. Set them in `workbench.local.yaml` to mark a project differently on your machine. See Name, color, and icon in Several projects.
- `server.js` takes several project folders and serves each on its own port, with the same switcher in the browser.
- Agents can see the screen you have open without a paste. Every open canvas tells the Workbench server what it shows, and `GET /_workbench/view` returns the reference Copy reference would copy, from the canvas you changed most recently. In a project with a `.canonic` folder, the server writes its address to `.canonic/.workbench/server.json` while it runs; add that folder to `.gitignore`. See Copy a reference in Using the canvas.

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
