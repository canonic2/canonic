# Storybook

Workbench can show Storybook stories in its canvas without Storybook's own
interface: one story at a time, at a real device width, with annotations and
handoffs. There are two ways to use it, and you can combine them:

- **Map design pages to stories.** A design page gets a Storybook lens, so
  you can compare the design with the built component.
- **Import the whole catalog.** Every story title becomes a page, and each of
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

## Map design pages to stories

Give a design page the story **title** it corresponds to:

```yaml
collections:
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

With these defaults, the lens switcher shows **Design** and **Storybook**.
Set the implementation's `label` to rename **Storybook**, and the page's
`lensLabel` to rename **Design**. The implementation key and story address
stay the same. See [Customize lens labels](lenses.md#customize-lens-labels).
Choosing
Storybook shows the title's first story. The state switcher, after the page's
name in the top bar's breadcrumb, lists the title's stories instead of the
design's states. Switch between them without leaving the frame.

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

With `catalog: true`, every story title becomes a page, and you don't need
`collections`. This can be the whole `workbench.yaml`:

```yaml
name: Acme UI

implementations:
  storybook:
    kind: storybook
    url: auto
    catalog: true
```

How titles become the sidebar:

| Storybook title | Collection | Group | Page |
| --- | --- | --- | --- |
| `UI/Components/Button` | UI | Components | Button |
| `UI/Forms/Inputs/Text field` | UI | Forms / Inputs | Text field |
| `Auth/Sign in` | Auth | none | Sign in |
| `Button` | Storybook (the implementation's label) | none | Button |

- The first segment is the collection. A one-segment title goes in a
  collection named after the implementation.
- Middle segments become one group. Groups don't nest, so deeper levels are
  joined with ` / `.
- The last segment labels the page.
- Each story under a title is one of the page's states, labeled with the
  story's name. A title with one story is a page with no states listed under
  it.
- Only stories are imported. Storybook's docs entries are skipped.

Imported pages are Storybook-only: they open straight in the Storybook lens
and have no Design choice. Their source links come from Storybook's own
`importPath` and `componentPath`, resolved against `root`.

### Mixing imported and hand-written pages

`catalog` and `collections` work together. An imported collection with the same
name as a hand-written or preview collection is merged into it. Within that
collection, groups with exactly the same name share one group, with imported
pages added after the existing pages. You can put design pages and previews next
to their stories:

```yaml
implementations:
  storybook:
    kind: storybook
    url: http://localhost:6006
    catalog: true

collections:
  - name: Components          # Storybook's Components/* titles are added here too
    icon: component
    items:
      - label: Button (design)
        src: design/button.html
        implementations:
          storybook: Components/Button
```

### Catalog icons

By default, imported pages and collections use the `book-open` icon. Make
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
- If startup times out, the canvas still opens with your hand-written pages
  and reports the catalog problem.

Start commands run only in trusted workspaces, and only in VS Code. See
[Start commands](configuration.md#start-commands) for every key.

## Switching stories

The first story loads in an iframe at
`<url>/iframe.html?id=<story-id>&viewMode=story`, through the loopback proxy
Workbench runs for each implementation (see
[Embedding and sign-in](lenses.md#embedding-and-sign-in)). After that, Workbench
switches stories through Storybook's own channel, so the preview stays loaded
and switching is fast. If Storybook doesn't confirm a switch within about
1.5 seconds, or doesn't finish rendering within 15 seconds, Workbench loads the
story's URL instead.

Keyboard shortcuts reach the workbench through Storybook's key channel, so
canvas zoom and editor shortcuts keep working while a story has focus.
Storybook doesn't forward keys while a text field in the story is focused.

## Screenshots of stories

A screenshot of a story shows it as you see it, including anything you
changed by interacting: an opened modal or menu, typed text, a scroll position.
Your Storybook needs no setup for this. Workbench's proxy adds the
[preview bridge](lenses.md#screenshots-through-a-url-lens) to the story's page,
and the bridge sends the live document to the workbench. It doesn't send
cookies, storage, credentials, or code.

## Exports

A [design-system export](design-system-export.md) includes each story's source
and local imports, your Storybook configuration from `.storybook` or a
`--config-dir` named in a package script, and a reference screenshot of every
imported story at each of the page's `sizes`. Set `root` so sources can be
found.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| No Storybook lens on a page | The title is exact. Ask the [stories route](troubleshooting.md#check-a-storybook-title), which lists near matches. |
| Catalog is empty | Storybook isn't running, or isn't at `url`. The config route's `problems` says which. Open `<url>/index.json` in a browser. |
| `url: auto` finds nothing | Storybook isn't on a scripted port or 6006 to 6010. Set `url` explicitly, or in `workbench.local.yaml`. |
| Lens is blank | Open `<url>/iframe.html` directly, and check the frame's console. |
| No source links | `root` is missing or points at the wrong folder. It should be the folder Storybook runs from. |
| Screenshots don't match what you see | The story hadn't finished loading, so the bridge hadn't connected. Take the screenshot again. |
