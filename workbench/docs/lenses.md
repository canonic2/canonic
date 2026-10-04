# Lenses and URL implementations

A design page is one picture of something that also exists as code. A
**lens** shows that code's version in the design's place: the same artboard, the
same size, with your annotations over it. Switch between them to compare the
design with what was built, then hand the difference to an agent.

There are six kinds of implementation:

| Kind | Shows | Guide |
| --- | --- | --- |
| `url` | A page from any web server: your dev server, a preview deployment, staging | This page |
| `workbench` | A named TypeScript preview from this project | [Workbench previews](workbench-previews.md) |
| `storybook` | One Storybook story, without Storybook's own interface | [Storybook](storybook.md) |
| `ios-simulator` | A live, interactive stream of a booted iOS Simulator | [iOS Simulator](ios-simulator.md) |
| `window` | A live stream of a window from any macOS app, such as an Android emulator | [App windows](windows.md) |
| `examples` | A docs page's examples, rendered with an adapter such as React or React Native Web | [Docs pages](docs-pages.md#lenses) |

This page covers how lenses work in general, how to set up a `url`
implementation, and how to point a design page at a
[Workbench preview](#compare-a-design-with-a-workbench-preview).

## How lenses work

A page that lists any implementations gets a **lens switcher** in the
top bar: **Design**, then one button per implementation the page has. On a
page whose `src` is a `.workbench.ts` or `.workbench.tsx` file, the first
button reads **Workbench** instead. The switcher shows only when there are two
or more lenses to choose from, so a page imported from a single catalog, with
no design, has none.

- **The choice sticks** as you move between pages, like the artboard size.
  A page that doesn't have the chosen lens shows its design.
- **The address includes it.** `#pages/sign-in.html:error@393~staging` is the
  sign-in page in its error state, at mobile width, on staging. A copied link
  opens the same view. See [Links and the address](canvas.md#links-and-the-address).
- **Annotations, screenshots, and handoffs work through every lens.** A
  screenshot taken through a lens is named after it
  (`sign-in-error-staging.jpg`), and the handoff says which implementation it
  shows, at what URL, and where that implementation's code is.
- **Actions** is always on through a `url` or `storybook` lens. Your app
  serves that page, so the workbench can't stop its links and forms.
- **Open on its own** in the top bar opens the current page in your browser,
  outside the workbench, for interaction the iframe can't provide.

### Lenses on a docs page

A [docs page](docs-pages.md) has no design to compare with: its lenses are
`examples` implementations, and each one renders the page's examples, such as
with React or with React Native Web. The Markdown is the same in every lens,
and there is no **Design** button. The switcher shows when the page has two or
more lenses. A docs page that doesn't have the chosen lens shows its own
default lens. See [Lenses](docs-pages.md#lenses) in the docs page guide.

## Set up a URL implementation

### 1. Declare the implementation

Implementations are declared once, at the top of `workbench.yaml`:

```yaml
implementations:
  dev:
    kind: url
    base: http://127.0.0.1:3000
    root: ../acme-web
  staging:
    kind: url
    label: Staging
    base: https://staging.example.com
```

- The name (`dev`, `staging`) is kebab-case. It labels the lens button unless
  you set `label`.
- `base` is the server's origin, plus a base path if every page lives under
  one (`https://example.com/app`).
- `root` is where the implementation's code is, relative to `workbench.yaml` or
  absolute. It is optional, and only needed for [code pointers](#point-at-the-code).

### 2. Tell each page where it is

```yaml
collections:
  - name: Pages
    items:
      - label: Sign in
        src: design/sign-in.html
        implementations:
          dev: /sign-in
          staging: /sign-in
```

Paths start with `/` and are appended to `base`, so `dev` shows
`http://127.0.0.1:3000/sign-in`. A page only gets the lenses it lists.

### 3. Map states to paths

If the implementation can show a state through its URL, give the page a map
of state ids to paths instead of a single path:

```yaml
- label: Sign in
  src: design/sign-in.html
  states:
    - id: default
      label: Default
    - id: error
      label: Wrong password
    - id: locked
      label: Account locked
  implementations:
    dev:
      default: /sign-in
      error: /sign-in?error=invalid
      locked: /sign-in?error=locked
```

- The entry for the first state, or `default`, is required.
- A state without an entry uses the default path.
- Picking a state in the sidebar loads its path, so you can compare each design
  state with the implementation in the same state.

Query strings, fixtures, or mock flags are the usual way to do this. Many apps
already have them for tests.

### 4. Keep machine-specific values local

Ports and checkout locations differ between machines. Commit values that work
for most people, and override the rest in `workbench.local.yaml`, which is in
`.gitignore`:

```yaml
# workbench.local.yaml
implementations:
  dev:
    base: http://127.0.0.1:4000
    root: /Users/me/src/acme-web
```

Each implementation's keys are merged over the committed ones, so this keeps
`dev`'s `kind` and any `start` command. See
[Local overrides](configuration.md#local-overrides).

## Start the server automatically

A `url` implementation can name the command that starts it. When VS Code opens
the project, Workbench checks whether the server is answering, and only runs
the command if it isn't:

```yaml
implementations:
  dev:
    kind: url
    base: http://127.0.0.1:3000
    start:
      command: pnpm dev
      cwd: ../acme-web
      check:
        port: 3000
      timeout: 60
```

1. On activation, Workbench runs the `check`: a TCP connection to `port`
   (on `127.0.0.1` unless you set `host`), or an HTTP request to `url` that
   has to answer with a success status.
2. If the check passes, nothing else happens. An existing server is never
   started twice.
3. If it fails, Workbench opens a VS Code terminal in `cwd` (relative to
   `workbench.yaml`) and runs `command`. The terminal stays open, so you can
   read its output and stop it.
4. It waits up to `timeout` seconds for `ready`, or for `check` if `ready` is
   omitted. `timeout` defaults to 60 and can be up to 300. If the server
   isn't ready in time, the canvas still opens, and the **Workbench** output
   channel records that it didn't become ready.

Use `ready` when the server listens before it can serve:

```yaml
start:
  command: pnpm dev
  check:
    port: 3000
  ready:
    url: http://127.0.0.1:3000/health
  timeout: 120
```

Start commands run only in [trusted workspaces](https://code.visualstudio.com/docs/editor/workspace-trust),
and only in VS Code. A standalone `server.js` doesn't run them. A local
override replaces the whole `start` block, which is useful when your machine
starts the app differently. Every key is listed in
[Start commands](configuration.md#start-commands).

## Embedding and sign-in

A URL lens loads your app in an ordinary iframe, through a proxy Workbench
runs for each implementation origin. The proxy listens on its own loopback
port, such as `http://127.0.0.1:50353`, and passes every request and WebSocket
on to your app. Typing, scrolling, selection, and sign-in all happen in your
app's own page, and your app sees requests addressed to its own host, with
`Origin` and `Referer` to match. Nothing in your app needs to change.

On the way back, the proxy:

- adds one script to each page, the preview bridge, which sends the page's
  live document to the workbench for
  [screenshots](#screenshots-through-a-url-lens);
- drops headers that would keep the page out of a frame: `X-Frame-Options`,
  `Content-Security-Policy`, and the cross-origin opener, embedder, and
  resource policies;
- removes `Domain`, `Secure`, and `SameSite` from cookies, so they belong to
  the proxy's address;
- keeps redirects to your app's origin on the proxy.

### Keep the session

Sign in inside the lens, and sign out with the app's own logout. The lens has
its own browser session, separate from your regular browser. The workbench and
the proxy are both on `127.0.0.1`, so your app's cookies are first-party in the
lens.

- Sign-in that goes through an identity provider returns to the callback
  address registered with it: your app's own origin, not the proxy. The lens
  doesn't keep that session. Use a sign-in your development build serves
  itself.
- Links and scripts that spell out your app's full origin, such as
  `http://localhost:3000/settings`, leave the proxy. Relative links stay on it.

When a page fails to load, the failure stays in the frame; use **Open on its
own** to compare. If the app isn't running, the proxy answers that it isn't
answering.

## Screenshots through a URL lens

Screenshots and handoffs of a URL lens show the page as you see it: open
dialogs and menus, form values, and scroll positions included. The preview
bridge the proxy adds to each page sends its live document to the workbench,
which renders it in the screenshot helper.

- The bridge sends the visible document: DOM, open shadow roots, styles, form
  values, and scroll positions. It doesn't send cookies, storage,
  credentials, or code.
- It answers only a workbench on a loopback address, and the workbench accepts
  its messages only from the active iframe.

If the bridge can't connect, for example because the page hasn't finished
loading, the screenshot helper loads the same URL itself, in its own session,
and lays your annotations over the result. That copy starts fresh: it doesn't
include what you did in the page, and a page behind sign-in may show its
sign-in page.

## Compare a design with a Workbench preview

A `workbench` implementation shows one of this project's
[TypeScript previews](workbench-previews.md) in place of a design. It takes no
`base`, `start`, or `root` of its own: previews come from this project.

```yaml
implementations:
  implementation:
    kind: workbench
    label: Implementation

collections:
  - name: Components
    items:
      - label: Button
        src: design/button.html
        implementations:
          implementation: components/button
```

- The value is the preview's `id`, such as `components/button`. An unknown ID
  is reported by the [config route](troubleshooting.md#read-the-resolved-config).
- A design state loads the preview state with the same id. Any other state
  shows the preview's first state.
- **Open the source** lists the preview's source file, and the handoff names
  it.
- Previews run project code, so they need a
  [trusted workspace](https://code.visualstudio.com/docs/editor/workspace-trust).

## Point at the code

`code` tells Workbench where a page's implementation lives, so the editor can
open it and handoffs can name it:

```yaml
implementations:
  dev:
    kind: url
    base: http://127.0.0.1:3000
    root: ../acme-web

collections:
  - name: Pages
    items:
      - label: Sign in
        src: design/sign-in.html
        implementations:
          dev: /sign-in
        code:
          dev:
            - src/routes/sign-in/page.tsx
            - src/components/sign-in-form.tsx
```

- Paths are relative to the implementation's `root`, or absolute. Folders work
  as well as files. The implementation needs a `root` either way; without
  one, its code pointers are listed but can't be opened, and the config route
  reports the problem.
- **Open the source** (`</>`) in the top bar lists the design file and each code
  pointer, and opens the one you pick in the editor. A file opens in a tab; a
  folder is revealed in the Explorer, or in your file browser when it's outside
  the window. Opening files needs the extension.
- The handoff includes a `Source:` line with the absolute paths, and the
  page's design file. An agent reading it knows which code to change and
  which design to match. Code pointers appear in the handoff even on the Design
  lens.
- Paths are resolved on your machine. A path that doesn't exist is shown
  disabled in **Open the source**, left out of the handoff, and reported by
  the [config route](troubleshooting.md#read-the-resolved-config) with
  `exists: false`.

Code pointers don't need a lens. A page can list `code` for an
implementation without listing it under its own `implementations`. It then
gets source links without a lens button.

## Examples

### A dev server and staging, with per-state paths on dev

```yaml
implementations:
  dev:
    kind: url
    base: http://127.0.0.1:5173
    root: .
    start:
      command: npm run dev
      check:
        port: 5173
  staging:
    kind: url
    base: https://staging.example.com

collections:
  - name: Pages
    items:
      - label: Checkout
        src: design/checkout.html
        states:
          - id: default
            label: Cart
          - id: payment
            label: Payment
        implementations:
          dev:
            default: /checkout
            payment: /checkout?step=payment
          staging: /checkout
        code:
          dev: src/pages/checkout
```

### An app under a base path

```yaml
implementations:
  admin:
    kind: url
    base: http://127.0.0.1:8080/admin
```

A page path of `/users` loads `http://127.0.0.1:8080/admin/users`.
