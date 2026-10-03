# Lenses and URL implementations

A design screen is one picture of something that also exists as code. A
**lens** shows that code's version in the design's place: the same frame, the
same width, with your marks over it. Switch between them to compare the
design with what was built, then hand the difference to an agent.

There are five kinds of implementation:

| Kind | Shows | Guide |
| --- | --- | --- |
| `url` | A page from any web server: your dev server, a preview deployment, staging | This page |
| `workbench` | A named TypeScript preview from this project | [Workbench previews](workbench-previews.md) |
| `storybook` | One Storybook story, without Storybook's own interface | [Storybook](storybook.md) |
| `ios-simulator` | A live, interactive stream of a booted iOS Simulator | [iOS Simulator](ios-simulator.md) |
| `window` | A live stream of a window from any macOS app, such as an Android emulator | [App windows](windows.md) |

This page covers how lenses work in general, how to set up a `url`
implementation, and how to point a design screen at a
[Workbench preview](#compare-a-design-with-a-workbench-preview).

## How lenses work

A screen that lists any implementations gets a **lens switcher** in the
toolbar: **Design**, then one button per implementation the screen has. On a
screen whose `src` is a `.workbench.ts` or `.workbench.tsx` file, the first
button reads **Workbench** instead.

- **The choice sticks** as you move between screens, like the frame width.
  A screen that doesn't have the chosen lens shows its design.
- **The address includes it.** `#pages/sign-in.html:error@393~staging` is the
  sign-in screen in its error state, at mobile width, on staging. A copied link
  opens the same view. See [Links and the address](canvas.md#links-and-the-address).
- **Markup, screenshots, and handoffs work through every lens.** A screenshot
  taken through a lens is named after it (`sign-in-error-staging.jpg`), and the
  handoff says which implementation it shows, at what URL, and where that
  implementation's code is.
- **Actions** is always on through a `url` or `storybook` lens. Your app
  serves that page, so the workbench can't stop its links and forms.
- **Open on its own** in the toolbar opens the current page in your browser,
  outside the workbench, for interaction the iframe can't provide.

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

### 2. Tell each screen where it is

```yaml
sections:
  - name: Pages
    items:
      - label: Sign in
        src: design/sign-in.html
        implementations:
          dev: /sign-in
          staging: /sign-in
```

Paths start with `/` and are appended to `base`, so `dev` shows
`http://127.0.0.1:3000/sign-in`. A screen only gets the lenses it lists.

### 3. Map states to paths

If the implementation can show a state through its URL, give the screen a map
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

A URL lens loads your app in an ordinary iframe. Typing, scrolling, selection,
and sign-in all happen in that page. For that to work, your app's development
environment has to allow being framed, and its session has to work inside a
frame.

### Allow framing

If the lens stays blank, or the browser console reports `X-Frame-Options` or
`frame-ancestors`, the app refuses to be framed:

- In development, the simplest fix is not to send `X-Frame-Options` and not to
  restrict `frame-ancestors`.
- If you need a policy, `frame-ancestors` must allow **every** ancestor. In a
  standalone browser that is the workbench, `http://127.0.0.1:3579` (or the
  port it reports). Inside VS Code the workbench is itself inside the editor's
  webviews, which are further ancestors. The console message names the
  ancestor that was refused.
- Keep production policies as they are. Change development and preview
  environments only.

See [frame-ancestors on MDN](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Content-Security-Policy/frame-ancestors).

### Keep the session

Sign in inside the lens, and sign out with the app's own logout. The lens has
its own browser session, separate from your regular browser and from the
screenshot helper. If sign-in doesn't stick:

- **Use the same host name everywhere.** `localhost` and `127.0.0.1` are
  different sites, so cookies set on one aren't sent to the other.
- **Cookies in a frame are third-party when the frame is cross-site.** They
  need `SameSite=None; Secure`, and browser restrictions on third-party
  cookies can still block them. Inside VS Code, the editor's own webview is the
  top-level page, so expect the iframe to be treated as cross-site.
- **Some identity providers refuse to be framed.** Use a popup or redirect
  sign-in flow your app supports, so the session is established in the lens's
  browser context. Signing in from your regular browser doesn't share the
  session.

See [third-party cookies on MDN](https://developer.mozilla.org/en-US/docs/Web/Privacy/Guides/Third-party_cookies).
When embedding fails, the failure stays in the frame. Workbench doesn't switch
to another way of showing the page; fix the app's configuration or use
**Open on its own**.

## Screenshots through a URL lens

A URL lens shows another origin's page, so the workbench can't read it
directly. Screenshots of it work in one of two ways.

**Without the preview bridge**, the screenshot helper loads the same URL
itself, in its own browser session, and lays your marks over the result. This
works for public pages. For a page behind sign-in, the helper isn't signed in,
so the screenshot may show your sign-in page. Form input, scroll positions, and
anything you changed by interacting are also not reflected.

**With the preview bridge**, the page sends its live document to the workbench,
which captures exactly what you see, including form values and scroll positions.
To opt in, load `/_workbench/preview-bridge.js` from the workbench's origin in
your app's development build, only when it is framed by a local workbench:

```js
if (typeof window !== 'undefined' && window.parent !== window && document.referrer) {
  const parent = new URL(document.referrer);
  if (['127.0.0.1', 'localhost', '[::1]'].includes(parent.hostname)) {
    const script = document.createElement('script');
    script.src = `${parent.origin}/_workbench/preview-bridge.js`;
    document.head.appendChild(script);
  }
}
```

- Browsers remove the path from a cross-origin referrer, so the check is on
  the loopback host, not on `/_workbench/`. The bridge repeats it for every
  message.
- The bridge sends the visible document: DOM, open shadow roots, styles, form
  values, and scroll positions. It doesn't send cookies, storage,
  credentials, or code.
- The workbench accepts bridge messages only from the active iframe, at the
  implementation's configured origin.

## Compare a design with a Workbench preview

A `workbench` implementation shows one of this project's
[TypeScript previews](workbench-previews.md) in place of a design. It takes no
`base`, `start`, or `root` of its own: previews come from this project.

```yaml
implementations:
  implementation:
    kind: workbench
    label: Implementation

sections:
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

`code` tells Workbench where a screen's implementation lives, so the editor can
open it and handoffs can name it:

```yaml
implementations:
  dev:
    kind: url
    base: http://127.0.0.1:3000
    root: ../acme-web

sections:
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
- **Open the source** (`</>`) in the toolbar lists the design file and each code
  pointer, and opens the one you pick in the editor. A file opens in a tab; a
  folder is revealed in the Explorer, or in your file browser when it's outside
  the window. Opening files needs the extension.
- The handoff includes a `Source:` line with the absolute paths, and the
  screen's design file. An agent reading it knows which code to change and
  which design to match. Code pointers appear in the handoff even on the Design
  lens.
- Paths are resolved on your machine. A path that doesn't exist is shown
  disabled in **Open the source**, left out of the handoff, and reported by
  the [config route](troubleshooting.md#read-the-resolved-config) with
  `exists: false`.

Code pointers don't need a lens. A screen can list `code` for an
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

sections:
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

A screen path of `/users` loads `http://127.0.0.1:8080/admin/users`.
