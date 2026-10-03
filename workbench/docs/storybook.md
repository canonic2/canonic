# Storybook

Workbench can show Storybook stories in its canvas without Storybook's own
interface: one story at a time, at a real device width, with markup and
handoffs. There are two ways to use it, and you can combine them:

- **Map design screens to stories.** A design screen gets a Storybook lens, so
  you can compare the design with the built component.
- **Import the whole catalog.** Every story title becomes a screen, and each of
  its stories becomes a state. No design pages or hand-written list are needed.

Workbench reads Storybook's live `/index.json`, which Storybook 7 and later
serve. Storybook has to be running, either started by you or by
[Workbench](#start-storybook-automatically).

## Declare the Storybook

```yaml
implementations:
  storybook:
    kind: storybook
    url: http://localhost:6006
    root: packages/ui
```

| Key | Description |
| --- | --- |
| `kind` | `storybook`. |
| `url` | Storybook's origin, or `auto` to [detect it](#detect-the-port-with-url-auto). |
| `root` | The folder Storybook runs from: the one containing `.storybook/`, relative to `workbench.yaml` or absolute. Optional, but needed for source links and exports. |
| `label` | The lens button's label. Defaults to the name in sentence case, *Storybook*. |
| `catalog` | `true`, or a map of icons, to [import every story](#import-the-whole-catalog). |
| `start` | A command that [starts Storybook](#start-storybook-automatically). |

The implementation name, `storybook` here, is any kebab-case key. Declare
several if you have several Storybooks, such as `web-ui` and `mobile-ui`.

## Map design screens to stories

Give a design screen the story **title** it corresponds to:

```yaml
sections:
  - name: Components
    items:
      - label: Button
        src: design/button.html
        implementations:
          storybook: Components/Button
```

The title is matched exactly, including its prefix and capitalization. It is
the `title` of the stories file, or the path Storybook generated for it, as
shown in Storybook's sidebar with `/` between levels. `Button` doesn't match
`Components/Button`. If only the prefix differs, the
[stories route](troubleshooting.md#check-a-storybook-title) suggests the right
title.

On that screen, the lens switcher shows **Design** and **Storybook**. Choosing
Storybook shows the title's first story. The **State** menu, after the screen's
name in the toolbar, lists the title's stories instead of the design's states.
Switch between them without leaving the frame.

The address records the story in the state slot, as the part of its id after
`--`: the story `components-button--icon-only` is
`#design/button.html:icon-only~storybook`.

## Import the whole catalog

```yaml
implementations:
  storybook:
    kind: storybook
    url: http://localhost:6006
    root: packages/ui
    catalog: true
```

With `catalog: true`, every story title becomes a screen, and you don't need
`sections`. This can be the whole `workbench.yaml`:

```yaml
name: Acme UI

implementations:
  storybook:
    kind: storybook
    url: auto
    catalog: true
```

How titles become the sidebar:

| Storybook title | Section | Folder | Screen |
| --- | --- | --- | --- |
| `UI/Components/Button` | UI | Components | Button |
| `UI/Forms/Inputs/Text field` | UI | Forms / Inputs | Text field |
| `Auth/Sign in` | Auth | none | Sign in |
| `Button` | Storybook (the implementation's label) | none | Button |

- The first segment is the section. A one-segment title goes in a section named
  after the implementation.
- Middle segments become one folder. Workbench folders don't nest, so deeper
  levels are joined with ` / `.
- The last segment labels the screen.
- Each story under a title is one of the screen's states, labeled with the
  story's name. A title with one story is a single row.
- Only stories are imported. Docs entries are skipped.

Imported screens are Storybook-only: they open straight in the Storybook lens
and have no Design choice. Their source links come from Storybook's own
`importPath` and `componentPath`, resolved against `root`.

### Mixing imported and hand-written screens

`catalog` and `sections` work together. Hand-written sections stay exactly as
written. An imported section with the same name as a hand-written one is
merged into it, so you can put design pages next to the stories they became:

```yaml
implementations:
  storybook:
    kind: storybook
    url: http://localhost:6006
    catalog: true

sections:
  - name: Components          # Storybook's Components/* titles are added here too
    icon: component
    items:
      - label: Button (design)
        src: design/button.html
        implementations:
          storybook: Components/Button
```

### Catalog icons

By default, imported screens and sections use the `book-open` icon. Make
`catalog` a map to choose icons by title prefix. The longest matching prefix
wins, so a specific title can override its category:

```yaml
catalog:
  icon: book-open                        # fallback
  icons:
    UI: palette
    UI/Components: component
    UI/Components/Button: mouse-pointer-click
    UI/Modules: boxes
    Auth: shield-check
```

Values are [Lucide](https://lucide.dev/icons/) icon names in kebab-case.

## Detect the port with `url: auto`

`url: auto` finds a running Storybook instead of hard-coding its port, which
is useful when different machines or worktrees use different ports:

1. Workbench looks through `package.json` files in the project, up to four
   folders deep and skipping `node_modules` and `.git`, for Storybook scripts
   that set `-p` or `--port`.
2. It tries those ports, then 6006 to 6010, on `127.0.0.1` and `localhost`, and
   uses the first one that serves `/index.json`.
3. If none answers, the config route and the sidebar report that no running
   Storybook was found.

Detection never starts Storybook. Combine it with [`start`](#start-storybook-automatically)
to have Workbench bring it up. The detected address is remembered while the
server runs, so if Storybook moves to another port, reload the window.

An explicit `url` in `workbench.local.yaml` overrides `auto` on one machine.

## Start Storybook automatically

```yaml
implementations:
  storybook:
    kind: storybook
    url: http://localhost:6006
    catalog: true
    start:
      command: pnpm storybook --no-open
      cwd: packages/ui
      check:
        port: 6006
      ready:
        url: http://localhost:6006/index.json
      timeout: 120
```

When VS Code opens the project, Workbench checks the port. If Storybook isn't
running, it opens a terminal in `cwd`, runs the command, and waits for `ready`
before importing the catalog.

- Use `ready` with the `/index.json` URL. Storybook listens on its port before
  it has finished building, and the index is what the catalog needs.
- Raise `timeout` for large Storybooks. The maximum is 300 seconds.
- Pass `--no-open` (or `--ci`) so Storybook doesn't open a browser tab.
- If startup times out, the canvas still opens with your hand-written screens
  and reports the catalog problem.

Start commands run only in trusted workspaces, and only in VS Code. See
[Start commands](configuration.md#start-commands) for every key.

## Switching stories

The first story loads in an iframe at
`<url>/iframe.html?id=<story-id>&viewMode=story`. After that, Workbench
switches stories through Storybook's own channel, so the preview stays loaded
and switching is fast. If Storybook doesn't confirm a switch within about
1.5 seconds, or doesn't finish rendering within 15 seconds, Workbench loads the
story's URL instead.

Keyboard shortcuts reach the workbench through Storybook's key channel, so
canvas zoom and editor shortcuts keep working while a story has focus.
Storybook doesn't forward keys while a text field in the story is focused.

## Screenshots of stories

Without extra setup, a screenshot of a story is taken by the screenshot helper
loading the story's URL itself, with your marks laid over it. Anything you
changed by interacting with the story, such as typed text, an opened menu, or a
scroll position, isn't included.

To capture exactly what you see, load the
[preview bridge](lenses.md#screenshots-through-a-url-lens) in
`.storybook/preview.ts` (or `preview.js`), only when a local workbench frames
the story:

```ts
// .storybook/preview.ts
if (typeof window !== 'undefined' && window.parent !== window && document.referrer) {
  const parent = new URL(document.referrer);
  if (['127.0.0.1', 'localhost', '[::1]'].includes(parent.hostname)) {
    const script = document.createElement('script');
    script.src = `${parent.origin}/_workbench/preview-bridge.js`;
    document.head.appendChild(script);
  }
}
```

The bridge sends the visible document, form values, and scroll positions to
the workbench. It doesn't send cookies, storage, credentials, or code, and it
only answers a workbench on a loopback address.

## Exports

A [design-system export](design-system-export.md) includes each story's source
and local imports, your Storybook configuration from `.storybook` or a
`--config-dir` named in a package script, and a reference screenshot of every
imported story at each of the screen's viewports. Set `root` so sources can be
found.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| No Storybook lens on a screen | The title is exact. Ask the [stories route](troubleshooting.md#check-a-storybook-title), which lists near matches. |
| Catalog is empty | Storybook isn't running, or isn't at `url`. The config route's `problems` says which. Open `<url>/index.json` in a browser. |
| `url: auto` finds nothing | Storybook isn't on a scripted port or 6006 to 6010. Set `url` explicitly, or in `workbench.local.yaml`. |
| Lens is blank | Open `<url>/iframe.html` directly. Check the console for framing errors from custom headers. |
| No source links | `root` is missing or points at the wrong folder. It should be the folder Storybook runs from. |
| Screenshots don't match what you see | Add the preview bridge. |
