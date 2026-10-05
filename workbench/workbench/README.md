# Workbench

A browser for the pages a project is designing: pick one in the sidebar, see
it at a real device width, draw on it, and hand the picture to an agent.

Nothing in this folder knows which project it is in. `workbench.yaml` configures
its spaces; pages come from declared pages, discovered TypeScript preview
definitions, and configured implementation catalogs.

This folder ships inside the Workbench extension and is served from there, so no
project holds a copy and no page in one points at it. Its browser entry points are HTML and script files, with TypeScript modules
stripped and served by the local server. No maintained compiled copy or
runtime CDN is required. Lucide ships with the
extension as a pinned dependency.

The [user documentation](../docs/README.md) is the guide for people setting
Workbench up. The [workbench specifications](../specs/README.md) record the
observable contracts for shared behavior, TypeScript previews, Storybook, and
other implementations.

---

## Multiple-artboard support

The [module layout contract](../specs/modules.md) defines target locations for
new and substantially migrated capabilities under `src/modules/`, alongside
shared components, themes and server infrastructure. The paths below describe
the current implementation.

The default `/_workbench/` route continues to serve `index.html` with its
existing controls and layout. TypeScript modules under `../src/canvas/` supply
serializable multi-artboard state, geometry, a controller with injected runtime
ports, authenticated iframe transport, full-canvas review, and context publication.
They do not install a replacement canvas UI. Shell and annotation ports expose the
existing renderer to a future host; native producers are pooled by source/codec.

See [the support-system record](../specs/multiple-artboards.md) and
[the module plan](../specs/multiple-artboards-code-plan.md) for APIs and limits.

## Putting it in a project

1. Install the Workbench extension.
2. Write `workbench.yaml` at the project root (schema below).

That is the whole of it. Pages need nothing added to preview well: the
server puts one `preview-compat.js` bundle into served HTML and managed
previews (including Astro). It bundles `keys.js`, `actions.js` and `states.js`:
keyboard forwarding, action handling, and page states, to keep navigation
inside Workbench and make `?state=…` mean something. A page opened straight
off the filesystem gets no injection, so it previews as authored and its links
behave like links.

## workbench.yaml

```yaml
name: Acme                    # the sidebar header, the tab title, and the space switcher
color: green                  # optional: the space's mark — a named colour or a hex value
icon: brand/logo.svg          # optional: a Lucide icon name, or a project image (up to 256 KB)

# spaces:                     # optional: several spaces in this file, keyed by id. Each takes
#   design-system:            # name, color, icon, collections, previews, implementations, and a root
#     root: packages/ui       # relative to this file; every other top-level key is shared, and a
#     collections: [...]      # space's implementations merge over the shared ones by name

previews:                     # optional: TypeScript preview discovery; false turns it off
  lensLabel: Design           # optional: authored preview lens label; default Workbench
  icon: component             # fallback for preview pages and collections
  icons:                      # longest matching title prefix wins
    Pages: monitor
  include:                    # project-relative globs; default **/*.workbench.ts and **/*.workbench.tsx
    - src/**/*.workbench.ts
  config: workbench.config.ts # project-relative adapter and compiler config; this is the default

implementations:              # optional: lenses, declared once and named by pages
  web:
    kind: examples            # renders docs page examples; also url, storybook, workbench, ios-simulator, window
    adapter: react            # html, react, vue, react-native-web, or one from workbench.config.ts
    styles:                   # optional, project-relative
      - src/theme.css

collections:                  # one entry in the collection list, in this order
  - name: Pages
    icon: file-text           # any kebab-case Lucide icon name
    items:
      - group: Auth           # optional; groups don't nest
        items:
          - label: Sign in
            src: pages/sign-in.html    # relative to the project root
            lensLabel: Reference      # optional: authored lens label; default Design for HTML
            sizes:                    # one or more supported artboard sizes
              - laptop
              - mobile
              - resizable             # resizable artboard; skipped in reference exports
            states:                    # optional; the first is the page as authored
              - id: default
                label: Default
              - id: error
                label: Wrong password

  - name: Components
    icon: component
    items:
      - label: Button
        src: components/button.html
      - label: Card            # a docs page: Markdown with live examples, filling the canvas
        src: docs/card.md
        lens: web             # optional: the lens it opens with; the first otherwise
        implementations:
          web: src/card/examples/     # a folder (one example per file) or a file (one per named export)
```

TypeScript previews in `.workbench.ts` and `.workbench.tsx` files are
discovered without being listed. Each definition's title places it: the first
segment is a collection (`Previews` when there is only one segment), middle
segments one group, and the last the page's label; its states come from the
definition. `previews: false` turns discovery and execution off. Definitions
run project code, so they need a trusted workspace. A handwritten page may
name a definition as its `src`; it keeps its place in `collections` and takes its
states from the definition. See [TypeScript previews](../docs/workbench-previews.md)
for definitions, the HTML, React, Vue, Astro and React Native Web adapters,
controls, plugins, and portable exports. `preview/compiler.cjs` discovers and
compiles them in a worker started by `preview-service.js`.

State ids are kebab-case — they travel in a URL and in a screenshot's filename.
Declaring one here is half of it: the page has to answer to the id, which
`states.js` explains and does. A `src` can't hold `:`, `!`, or `~`, which mark the
state, a docs page's example, and the lens in the address.

A `src` ending in `.md` is a docs page: its Markdown places examples with
fenced `example <id>` blocks, its `examples` lenses render them, and it fills the
canvas instead of an artboard. A `.workbench.ts` file can declare one with
`defineDocs`. See [Docs pages](../docs/docs-pages.md); the server and page code
are in `src/docs/`.

`sizes` controls both the sizes the size switcher offers for a page and its
reference images in a design-system export. A page lists keys from its space's
sizes, including custom sizes; omitted `sizes` supports every size in the space.
Unknown keys are reported. Fixed sizes capture their dimensions, filled axes use
1440 wide or 900 tall, and Fit captures 1440 × 900. Resizable enables an editable
artboard and is skipped in reference exports; a page whose only size is Resizable
gets one 1440 × 900 reference. Duplicate dimensions are captured once. See the
[sizes contract](../specs/sizes.md#design-system-export).

Use **Configure pages** in the top bar’s More menu to add or remove collections and
pages, edit labels and source paths, and select supported sizes. Saving
rewrites only the `collections` block in `workbench.yaml`; implementations and
comments outside that block are preserved. Advanced state, implementation, and
code mappings remain in the YAML and survive form edits.

### Implementations

A page can also be seen through lenses onto its implementation. They are
declared once at the top and referred to by name:

```yaml
implementations:
  storybook:                  # any kebab-case name; it's the button's label unless `label` says otherwise
    kind: storybook           # workbench | url | storybook | ios-simulator | window
    url: auto                 # or an explicit http://localhost:6006
    root: ../product/packages/ui   # optional folder: where the code is, relative to this file or absolute
    catalog: true             # optional: import its titles and stories into the workbench
    start:                    # optional: start it in VS Code when the check fails
      command: yarn storybook
      cwd: ../product         # relative to workbench.yaml; default is .
      check:
        port: 6006            # optional host defaults to 127.0.0.1
      ready:
        url: http://localhost:6006/index.json  # optional, for full service readiness
      timeout: 90             # seconds to wait for readiness; default 60
  simulator:
    kind: ios-simulator
    device: booted            # every booted Simulator, or one exact name/UDID
    catalog: true
  emulator:
    kind: window              # a live stream of one macOS app's window
    app: com.example.emulator # bundle ID, or part of it; a page names the window title
  staging:
    kind: url
    base: https://staging.example.com
  implementation:
    kind: workbench           # this project's TypeScript previews: no url, start, or other root
    label: Implementation

collections:
  - name: Pages
    items:
      - label: Sign in
        src: pages/sign-in.html
        implementations:
          staging: /          # url kind: one path for every state…
          dev:                # …or a map of this page's state ids to paths
            default: /
            error: /?error=1
        code:                 # implementation -> path or list of paths, relative to its root
          dev: packages/auth/src/pages/login
      - label: Button
        src: components/button.html
        implementations:
          storybook: Components/Button    # storybook kind: a title; its stories are read from the index
          implementation: components/button   # workbench kind: a preview ID
```

A `workbench` implementation's `root` can only be the project itself, and the
preview's definition file is added to the page's code pointers. An unknown
preview ID is reported against the page.

`workbench.local.yaml` beside it is merged over: each implementation by
name, every other top-level key whole. A missing one is the usual case; a
broken one is reported by its own name. Ports and folder paths go there, and
it goes in `.gitignore`.

`manifest.js` holds these rules, and the server reads the same file with
them, so the two never disagree about which lens a page has.

Every lens has a customizable display label. Implementations of every kind
use `label`; authored pages use `lensLabel`. For discovered TypeScript previews,
`previews.lensLabel` supplies the fallback, and an explicit page's `lensLabel`
takes precedence. Defaults remain **Design** for HTML and **Workbench** for
TypeScript previews. Renaming the authored lens changes its existing button
and review label, without adding a lens or changing addresses. Docs pages have
no authored lens and use their implementations' labels.

With `catalog: true`, a Storybook implementation can be the whole workbench;
`collections` may be omitted. Workbench reads Storybook's live `/index.json`, turns
each title into a page and each story under that title into a workbench state.
The first title segment becomes a collection, any middle segments become one
group, and the last segment labels the page. Generated pages open directly
through Storybook and use its component and story paths as source pointers when
`root` is configured. Handwritten collections can coexist with imported catalogs and are
left unchanged. Without `catalog: true`, no Storybook content is imported.

`catalog` may instead be a map. `icon` is the fallback Lucide icon, and
`icons` maps Storybook title prefixes to icons. The longest match wins, so an
exact title can override its category:

```yaml
    catalog:
      icon: book-open
      icons:
        UI: palette
        UI/Components: component
        UI/Components/Button: mouse-pointer-click
        UI/Modules: boxes
        Auth: shield-check
```

Handwritten pages may likewise set `icon: panel-top`; otherwise they use
their collection's icon.

Preview definitions may set `icon`; it overrides the longest `previews.icons`
title prefix, then `previews.icon`, then `component`. Prefixes match complete
segments separated by `/`. Collection lookup uses the collection name, independently
of a page's own icon. Icon settings validate kebab-case syntax, not Lucide
registry membership.

An authored collection may contain only `name` and `icon` to style imported
pages; the configuration editor preserves it, and it is hidden until filled.
Collection icon precedence is: explicit handwritten icon, preview prefix mapping,
catalog prefix mapping, configured preview fallback, configured catalog
fallback, built-in default. Shared preview/Storybook collections default to
`component`. Equal catalog priorities choose the alphabetically first icon
name, so import order never selects the icon. See the
[configuration reference](../docs/configuration.md#collections).
Authored collections without explicit icons keep `file-text` against imported
built-in defaults; mappings and configured fallbacks override it.

`start` is available for Storybook and URL implementations. On extension
activation, Workbench first checks the configured TCP port or HTTP(S) URL. If it
is already available, it leaves it running. Otherwise it opens a VS Code
terminal in `cwd` and runs `command`. It then waits up to `timeout` seconds
for `ready` (or `check` when `ready` is omitted) before importing the catalog.
For an HTTP check, put `url` under the check instead of `port`; only a
successful HTTP response counts as ready. Start commands run only in trusted workspaces.
If readiness times out, the workbench still opens and reports the catalog
problem. A local override may replace the whole `start` block in
`workbench.local.yaml`.

`url: auto` keeps Storybook opt-in while removing its machine-specific port.
The server reads Storybook package scripts for `-p`/`--port`, then checks those
ports and 6006–6010 for a live `/index.json`. If none answers, the config route
reports it and the editor sidebar shows the diagnostic. It only starts the
project's Storybook process when `start` is configured.

An `ios-simulator` implementation with `catalog: true` adds its matching booted
Simulator devices as implementation-only pages. In the extension, a small
ad-hoc-signed ScreenCaptureKit helper finds the matching Simulator window and
hardware-encodes a 30 FPS H.264 stream for the canvas. This avoids VS Code's
webview `display-capture` restriction. WDA forwards taps and drags from the canvas. Install WDA once in the project with
`node .canonic/src/automation/ios-cli.mjs install-wda`. The default `device:
booted` imports every booted device; an exact device name or UDID narrows it.

A `window` implementation uses the same helper for any macOS app: `app` is
part of its bundle ID, and each page maps it to part of a window title. The
canvas posts only the implementation and page to `/_workbench/window/stream`;
the server reads the app and title from the config, so a page can't open any
other window. Window lenses take no input, have no catalog or `start`, and
share the one native stream with the Simulator.

Whatever the config gets wrong is reported rather than guessed at. A bad line
names its line number; a page missing a `src` is dropped and logged with the
collection it was in, and the rest of the sidebar still builds.

## Serving

The extension serves this folder at `/_workbench/` and the project at `/`, on
one origin — `http://127.0.0.1:3579/_workbench/`, stepping up a port when one
is taken. Same origin lets the workbench read the live preview and synchronize
it into the warm native capture frame. A foreign origin prevents that. The config
is fetched too, which a `file://` page isn't allowed to do at all.

TypeScript previews are compiled by a worker process that `preview-service.js`
starts on the bundled Electron runtime's Node (plain Node from a source
checkout); the server proxies `/_workbench/previews/` and the definition files'
own paths to it. Managed previews use retained `preview-host.html`
documents. Each loads the compatibility bundle and `/_workbench/preview-runtime.js`
(`preview/browser.js`) once. `preview-host.js` fetches a compiled descriptor and
calls the module's adapter-neutral `mount` entry point; `browser.js` owns
cleanup of the adapter, canvas, event listeners, and revision timer. On
promotion the outgoing host remains attached and inert. `preview-sessions.js`
owns one active session and up to three inactive sessions, retained for 15
minutes after departure. Return preserves interactions and checks managed
source revisions; eviction disposes the host. Reload replaces the selected
session, and configuration refresh clears retained sessions. Library-owned
CSSOM sheets stay connected and are enabled only for the module using them.
Ordinary HTML and external lenses keep native document navigation. Portable
exports are self-contained.

The packaged extension starts its bundled Electron screenshot helper as soon
as the workspace's server starts. It stays running until the extension shuts
down. On macOS it is a background agent with no Dock icon or visible window;
users do not need to change how VS Code starts or install a capture browser.
Simulator streaming uses a separate **Canonic Window Capture** helper built
from the source shipped in the extension (`window-capture/`). It captures one
application's window, and the Simulator lens points it at the Simulator.
ScreenCaptureKit reads the window,
VideoToolbox encodes it once, and a loopback HTTP stream carries JPEG frames to
the embedded canvas without depending on VS Code's optional media codecs or
WebSocket forwarding. A standalone browser uses the lower-bandwidth
H.264/WebCodecs WebSocket path. Because the
extension launches the helper,
macOS attributes Screen Recording access to the editor's stable identity. Enable
**Visual Studio Code** (or the editor named by the error) in Screen & System Audio
Recording, restart the editor, then retry. The helper opens that settings pane
after a denial; there is no browser-capture path.
The runtime is unpacked into extension storage once and reused, without a
network download at runtime.

The workbench continuously mirrors the active local preview's live document,
open shadow roots, stylesheets, form values, scroll positions and annotations.
The first update supplies a complete set of node records. Subsequent updates
send only changed records, property states and deletions relative to the last
acknowledged revision. The helper applies them directly to a scriptless,
sandboxed document, retaining unchanged elements and decoded images. Application
code runs in the visible preview only. Canvas pixels, readable video frames, dialog/popover state and
animation phases are also transferred; styles are not computed for every node.
The mirror carries the pointer position, and the helper restores it before capture
so CSS hover menus paint as they do in the preview.
Mutations and input schedule coalesced updates without starving preparation on
animated pages. A 250 ms heartbeat checks for changes. Mutations invalidate
cached attributes; a lightweight traversal still checks live properties,
scroll positions, new shadow roots, inline/adopted CSSOM, media and animations.
This catches property assignments without events, without cloning the full DOM
or rebuilding and parsing an HTML string on every capture. An
unchanged view probes helper readiness every five seconds. Failures retry with backoff.
During a local capture, speculative synchronization pauses so a second DOM walk
does not compete with the current shot. It resumes with the latest state after
the capture finishes, including after failures. Mirror scroll restoration batches
reads before writes and leaves unchanged scroll positions alone.

Patches include changes since the acknowledged revision, including reversions
and deletions, so an intermediate background update does not invalidate a capture.
Reloading the preview resets its mirror. If the helper loses its base revision,
the workbench retries once with a full snapshot of the exact requested state;
later edits do not change the screenshot being retried. Capture flushes current
state and reads the prepared surface; changes still in flight must settle before
their image can be read. The first
mirror also needs its styles, fonts and images to load. This is not a guarantee
of constant capture latency for arbitrary pages.
Screenshots use JPEG at 90% quality for faster encoding. Dimensions match the
preview's CSS pixels, normalizing Retina bitmaps. The capture endpoints accept
`format: "jpeg"` or `format: "png"`; omitted format retains PNG for API callers.
JPEG is lossy and does not preserve transparency.

Interactive screenshots and handoffs use the continuously prepared, scroll-safe
single-view renderer. Design-system exports schedule up to four workers,
including the existing warm capture service: different pages or stories run in
parallel, while all sizes of one story stay on the same renderer. Export
settling and Storybook reuse do not navigate the visible preview.

Hosted captures report native failures instead of invoking the expensive
DOM-to-image renderer. Standalone `file://` use retains that renderer. Live
mirroring rejects nested iframes and embedded documents. Closed
shadow roots and browser-private UI are inaccessible; origin-tainted canvas or
video pixels cannot be transferred. Linked stylesheets load from their URLs;
runtime CSSOM edits to those sheets are not transferred. The mirror is not an
OS screen recording.
URL and Storybook lenses load through `../proxy.js`: one loopback proxy per
implementation origin, on its own port so absolute paths such as
`/@vite/client` keep working. The config route hands the browser the proxy
address (the original stays in `upstream`); the server keeps talking to the
implementation directly. The proxy passes requests and WebSockets through,
drops framing headers, rewrites cookies and redirects to its own origin, and
adds `/_workbench/preview-bridge.js` to the top of every HTML document a
navigation loads. The bridge transfers the visible document, including form
values and nested and viewport scroll, to the same inert capture mirror used
for local pages. The receiver accepts messages only from the active iframe at
its configured origin, and the preview bridge answers only a loopback parent.
It transfers DOM state, not code, cookies, storage or network credentials.

Until the bridge has sent its first snapshot, a lens capture falls back to the
helper loading the URL in its own session; signing in inside the iframe does
not sign the helper in, and that copy shows none of the reviewer's
interaction.

An unbundled source checkout or Linux host without a display uses the existing
headless Chrome/Chromium/Edge path. If the bundled helper cannot unpack or start,
the server retries through Chrome and keeps using it until the server stops.
Desktop macOS has been exercised; Windows/Linux runtime
and remote-workspace behavior still need platform validation. Browser-only
VS Code cannot spawn a local helper.

| How | Camera | Handoff | Config | Lenses |
| --- | --- | --- | --- | --- |
| The Workbench extension | downloads a JPEG | saved image + prompt copied to clipboard | yes | all, and open-in-editor |
| `node server.js <project>…` | downloads a JPEG | no | yes | all but open-in-editor |
| `file://…/index.html?root=…` | downloads a JPEG | no | needs `--allow-file-access-from-files` | the frame only: no story lookup |

Keep `.canonic/.handoffs/` in the project's `.gitignore`. Screenshots persisted
for handoffs are transient review artifacts rather than project source. The
standalone camera button does not write there.

Failures from the browser, capture server, and editor handoff are written to
VS Code's session-managed **Workbench** log. Open it with **Workbench: Show Log**. VS Code owns log retention; Workbench does not create an
unbounded project log, and each workbench server has a 2 MB session budget.
Diagnostic records are bounded metadata only and omit screenshots, mirrored
HTML, and handoff prompts.

Through a lens the page is another origin's, so the frame can't be read into.
A screenshot of it is taken by the screenshot helper going there itself
(`/_workbench/capture/page`), with the annotations laid over. The helper names what
is under each annotation with `describe.js`. URL and Storybook lenses use the
workbench's iframes. An explicitly configured `ios-simulator` lens instead uses
the native ScreenCaptureKit stream and WDA input path described above, and a
`window` lens uses the same stream without input.
After the first Storybook preview loads, Workbench switches stories through its
channel and keeps the preview runtime warm. It navigates to the story URL if
Storybook does not acknowledge the switch.

The last row is for working on the tool itself. `?root=` is how it is told
where the project is, since the tool ships in the extension rather than inside
the project. TypeScript previews need the server to compile them, so they
appear only in the first two rows. Where something can't work, the workbench
says so instead of coming up empty.

Use **Select** (or Escape to leave a drawing tool) to interact with the preview.
Selected annotations intercept input only on themselves and their handles; clicks
and text selection elsewhere reach the page immediately. In the editor, served
design pages copy and cut in their own document and forward right-click menus
with their selected text to VS Code. Editor shortcuts and chord continuations
are relayed; normal text editing and caret movement stay in the preview.
Standalone browser previews keep their browser shortcuts and menus. The chrome
itself is not selectable: select-all works in a field, a note, or the preview
page, and does nothing over the top bar, the toolbar, or the editor's wrapper around the artboard.

External iframe implementations remain subject to the browser's origin boundary.
The workbench grants clipboard access and uses Storybook's own key-event channel
when available, but cannot inject menu or clipboard handlers into an external
page. Storybook does not send those key events while an input is focused. The
**Open on its own** control opens the implementation directly for native browser
interaction in those cases.

## Embedding and sign-in

All design, URL, and Storybook lenses load in ordinary iframes. URL and
Storybook lenses come through the implementation proxy (see the capture
section above), which drops `X-Frame-Options`, `Content-Security-Policy`, and
cross-origin opener, embedder, and resource policies, and rewrites cookies to
its own origin. The proxy and the workbench share `127.0.0.1`, so an app's
cookies are first-party in the lens. Workbench does not assume VS Code shares
your normal browser's cookies. Sign in within the workbench's page, and use the
app's logout to end that session. An identity provider's callback returns to
the app's own origin rather than the proxy, so that session is not kept in the
lens.

A login or server failure stays in the iframe; the proxy answers 502 when the
implementation isn't running. Use **Open on its own**, which opens the
implementation's own address; Workbench never switches to a stream.

## The files

| File | Job |
| --- | --- |
| `index.html` | the shell: top bar, sidebar, canvas, and the toolbar and view controls floating over it |
| `config.js` | finds the project root, reads `workbench.yaml` and `workbench.local.yaml`, checks them |
| `config-editor.js` | edits collections, pages, paths, and supported sizes through the local server |
| `yaml.js` | the part of YAML a config is written in |
| `manifest.js` | the rules for implementations, lenses and code pointers — shared with the server |
| `address.js` | reads and writes the hash: src, state, width, lens |
| `lenses.js` | where a lens points: a url implementation's page, a single story |
| `describe.js` | names the element under a point — here, and inside pages the server's browser opens |
| `workbench.js` | routing, frame loading, resizing, the lens switcher, stories, the source menu |
| `reference.js` | copies the current page, state or story, and lens as a text reference |
| `top-bar.js` | builds the top bar’s More menu, and folds secondary actions into it before the bar’s regions collide |
| `preview-host.html` + `preview-host.js` | the reusable document a TypeScript preview mounts into |
| `preview-controls.js` + `preview-controls.css` | shared input controls, reset, actions and docs, also included in portable browser exports |
| `zoom.js` | zooms and pans the artboard on the canvas, Figma-style, and labels it with its name and size |
| `page-list.js` + `page-list.css` | the collection list and page list, laid out like Sketch's sidebar: collections, then the chosen one's groups, pages, and states, then search |
| `sidebar.html` + `sidebar.css` + `sidebar.js` | that same list in the editor's sidebar, in the editor's colours |
| `../src/components/space-switcher/` + `../src/components/space-mark/` | Web Components for the space switcher and its mark, with owned Shadow DOM styles |
| `../src/theme/defaults.css` | shared shell theme defaults and direct webview host mappings for migrated controls |
| `annotations.js` + `annotations.css` | the draw layer, screenshots, and clipboard handoff |
| `capture.html` + `capture.css` + `capture-page.js` | the minimal surface kept warm for compositor screenshots |
| `capture-sync.js` | coalesces preparation, checks readiness and retries failures |
| `dom-mirror.js` | caches node records, sends revisioned patches and applies them to the inert capture document |
| `preview-bridge.js` | external-preview sender for live DOM and scroll state, added to lens pages by the implementation proxy |
| `preview-bridge-client.js` | validates bridge messages from the active configured iframe |
| `workbench.css` | the chrome. Every value it uses is declared in it |
| `icons.js` | turns any configured Lucide name into workbench SVG markup |
| `modern-screenshot.js` | vendored DOM renderer for standalone file use |
| `simulator.js` | the canvas side of the native window stream, and Simulator input |
| `keys.js` | forwards editor shortcuts out of the workbench and its previews |
| `actions.js` | the preview's half of the Actions toggle, and the hook a TypeScript preview uses to claim its links and forms |
| `states.js` | the preview's half of page states |

`page-list.js` is the list and nothing else: it is handed the config's collections and
answers with a collection list and a page list that call back with the page you picked. Two
places build one — the workbench, where it is the left edge of the window, and
the Workbench sidebar in VS Code, where it stands in for a file tree. `page-list.css`
draws it in either place; it names no colours of its own, so `workbench.css`
dresses it in the tool's grays and `sidebar.css` in the editor's theme.

The `preview-compat.js` bundle, built by `../preview-scripts.js`, runs inside
previews served by Workbench. Its `actions.js` and `states.js` modules are
the reason those pages can apply shell state without project code: off
`file://` the frame is a foreign origin the shell can't reach into, so both
flags travel in the URL and the page applies them to itself. Nothing references
them — the server puts them into each page it serves, which is why a project's
plain pages stay plain HTML. Lens pages from another server get only
`preview-bridge.js`, from the implementation proxy: the compatibility bundle's
form and navigation handling would change how those apps behave.

## The address bar

**Copy reference** in the top bar copies a short text reference for pasting into
a conversation: the page's label and design file, its state (including the
default) or Storybook story, and the active lens with its implementation URL.
It uses the resolved selection and works in both the editor and the standalone
browser. Copying does not capture a screenshot, include annotations, or send a handoff.

**Download design-system ZIP** exports the whole current space across its
collections and pages. Other spaces are exported separately. An included
`workbench.yaml` is copied as written and may declare other spaces without
including their pages. It starts from every design file and resolved component, page,
and Storybook source pointer, follows local imports and referenced assets, and
preserves project-relative paths. The archive includes Storybook and package
configuration plus a manifest of files, external packages, unresolved
references, and one content record per page. Each record has a SHA-256 hash
over that page's sorted archive-relative paths and raw file contents. A
matching hash means the exported design/source entry points and their local
dependency closure are unchanged. Workspace files retain their project-relative
paths directly at the archive root, without a `project/` wrapper. Each page
also gets an agent-oriented README beside its primary component or page entry,
with its hash, entry points, included files, and usage guidance. Generated metadata and exporter-added package configuration are
outside the hash. The export excludes installed dependencies, build output,
secrets, tests, and source that is not reachable from the workbench. The button
requires the local workbench server.

The export also captures JPEGs for every declared design state and every
imported Storybook story at each page's supported `sizes`, except Resizable.
Fixed sizes use their dimensions and filled axes use 1440 wide or 900 tall.
A page whose only size is Resizable gets one 1440 × 900 reference. Duplicate
dimensions are captured once. References live in a
`screenshots/` directory beside that page's primary component or page entry
and are embedded in its README. When multiple pages share a source directory,
their README and screenshot paths include the page name to avoid collisions. They
are generated context rather than hash input. Capture runs as a background
export job; the workbench puts a modal progress bar in front of the canvas and
keeps the visible selection unchanged. A failed or unsupported reference does
not discard the archive: `captureWarnings` in `canonic-export.json` names it,
and the remaining references continue.

When the project has TypeScript previews, the archive also holds a standalone
browser viewer of them under `browser/`, built by `preview/portable.cjs`:
serve the extracted directory with any static HTTP server and open
`browser/index.html`. `canonic-export.json` lists its entries and build
warnings under `browser`. If the portable builder fails entirely, the sources
and references still download without `browser/`; `warnings` records the cause.

An export that fits in 10,000,000 bytes downloads as one `<design-system>.zip`.
A larger one downloads as one outer `<design-system>-parts.zip` holding
numbered ZIPs of at most 10,000,000 bytes each, for tools with a 10 MB
attachment limit. Parts are named `<design-system>-part-01-of-NN.zip`. Every part extracts
into the same top-level directory, repeats `README.md` and
`canonic-export.json`, and includes a `canonic-export-part-NN.json` file inventory.
Extract the outer bundle first. Upload its numbered ZIPs together, or extract all
of those parts into one directory when a conventional merged tree is needed.

Storybook configuration is included from `.storybook` or from a local
`--config-dir`/`-c` named by the implementation package's Storybook scripts.
Imports from linked local packages follow their `package.json` exports instead
of being treated as installed dependencies. A static
`new URL("./assets/", import.meta.url)` directory reference includes the files
under that directory; this covers build plugins that assemble sprites, fonts,
or other generated resources from an asset folder rather than importing each
asset individually. Those files also participate in the hashes of pages that
use that Storybook configuration.

Reachable package configuration is included too. When a Vite config names an
SVG sprite `iconDirs` directory through `path.resolve(process.cwd(), "…")`,
the export includes its source SVGs and counts them in the hashes of pages
whose components use `#icon-` symbols. The sprite itself is generated by Vite
at runtime. TypeScript source imports written with `.js` output extensions
resolve to their `.ts` or `.tsx` files when the JavaScript file is absent.

The selection lives in the hash, so a reload or a copied link lands on the same
page, in the same state, at the same width, through the same lens:

```text
#pages/sign-in.html:error@1512~staging
   └ src        └ state └ width └ lens — an implementation's name
                          fit, a device width, or resizable
```

The default state and the design lens are addressed by leaving them out. Under
a Storybook lens the state slot carries the story. The lens is the canvas's to
keep, like the width: a pick that names none keeps the one the lens switcher is on,
and a page without it shows its design. Once the canvas has settled, the
address is rewritten to say exactly what's showing.

**Fit** alone follows the available workbench space. Device presets keep their
declared width and height; **Resizable** likewise keeps its last manually
chosen dimensions. The page always lays out at that real size. What changes
with the window is the canvas zoom, as in a design tool: a new artboard size, or
a resized editor, zooms the artboard to fit (never past 100%) until you zoom or
pan by hand. `zoom.js` owns this, with Figma's controls — ⌘/Ctrl with the
wheel or a pinch zooms at the pointer, the wheel pans, Space-drag or the middle
button pans, ⌘= and ⌘- step by powers of two, ⌘0 and ⇧0 go to 100%, and ⇧1
fits. **Recenter view**, in the view controls, centers the artboard without
changing its zoom or the page's scroll position. Those chords stay with the
workbench rather than reaching the editor.
They are read inside previews the workbench serves, and through Storybook's
key channel for a Storybook lens; another origin's iframe keeps its own wheel.
Screenshots and handoffs are always taken at the artboard's real size, whatever
the zoom.

It is also how the workbench is driven from outside. Embedded in an editor —
the Workbench extension puts it in a tab with the page list in the sidebar —
a picked page arrives as a `wb-go` message carrying that same hash, from the
host and only the host, and the workbench answers every routing with a
`wb-here` saying which page and state it settled on — and which lens, which
the editor's list reads past. An editor pick and a copied link are the same
instruction, which is why there is only one of them to maintain, and
`wb-here` is what keeps the sidebar's selection current when the canvas moves on its own —
a link followed in a live preview.

Embedded, the workbench also drops its own page list: the list is in the
sidebar, and two of them would be one too many.

A workbench with several spaces runs one server per space, and the canvas
reads them from `/_workbench/spaces`. A space of a file that lists several,
or whose root isn't the file's folder, is served with
`<meta name="canonic-config">` (where the file is: `/_workbench/manifest/`
over http) and `<meta name="canonic-space">` (its id) in the canvas page;
`config.js` reads the file from there and `manifest.js`'s `selectSpace`
picks the space, the same rule the server uses. The editor's sidebar gets
the same two metas. With more than one, it puts the
space at the start of the breadcrumb and, in a browser, a switcher at the
top of its sidebar. Switching is going to the other server's address:
embedded, the canvas posts `wb-space` with the space's id and the host
loads that server in its tab; in a browser, it posts the id to
`/_workbench/spaces/open`, which starts that server if it has to and
answers with its URL. The editor's sidebar draws the same switcher from the
spaces the extension lists in its page.
