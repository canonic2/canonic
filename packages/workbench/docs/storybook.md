# Storybook

Workbench can show Storybook stories in its canvas without Storybook's own
interface: one story at a time, at a real device width, with annotations and
handoffs. You list the pages in `workbench.yaml` and map each one to a story
title: the page gets a Storybook lens, so you can compare the design with the
built component, and in that lens the scenario switcher lists the title's stories.

Workbench reads Storybook's live `/index.json`, which Storybook 7 and later
serve. Either Workbench runs your Storybook for you, or you run it and give
Workbench its address.

## Let Workbench run Storybook

```yaml
implementations:
  storybook:
    kind: storybook
    root: packages/ui
```

With no `url`, Workbench runs your project's own Storybook: the `storybook`
package and a framework installed in `root`, with your `.storybook/`
configuration, addons and builder. You don't start anything.

- It starts the first time you open a story, which takes a few seconds, and
  stops when Workbench closes. If it exits, the next story you open starts it
  again.
- It uses no port. Workbench reaches it over a private local socket, so it
  never conflicts with a Storybook you run yourself on 6006 or any other
  port, and nothing else can connect to it.
- Edits to your stories update the open story in place, as in Storybook.
- It runs your project's code, so it runs only in a trusted workspace.
- It starts with `storybook dev --preview-only`, without Storybook's own
  interface. Storybook versions without `--preview-only` start without it.

If it can't start, the lens shows why, with Storybook's own output. When
Storybook isn't installed in `root`, it says so.

## Use a Storybook you run

```yaml
implementations:
  storybook:
    kind: storybook
    url: http://localhost:6006
    root: packages/ui
```

With `url`, Workbench reads the Storybook at that address. Start it yourself,
or have VS Code [start it](#start-storybook-automatically).

| Key | Description |
| --- | --- |
| `kind` | `storybook`. |
| `url` | Optional. The origin of a Storybook you run, or `auto` to [detect it](#detect-the-port-with-url-auto). Leave it out to [let Workbench run Storybook](#let-workbench-run-storybook). |
| `root` | The folder Storybook runs from: the one containing `.storybook/`, relative to `workbench.yaml` or absolute. Without `url`, the folder Workbench runs it in, and the space's folder when left out. With `url`, optional, but needed for source links and exports. |
| `label` | The lens button's label. Defaults to the name in sentence case, *Storybook*. |
| `start` | With `url`: a command that [starts Storybook](#start-storybook-automatically). Without `url` it is reported, since Workbench runs Storybook itself. |

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
Storybook shows the title's first story. The scenario switcher, after the page's
name in the top bar's breadcrumb, lists the title's stories instead of the
design's scenarios. Switch between them without leaving the frame.

The address records the story in the scenario slot, as the part of its id after
`--`: the story `components-button--icon-only` is
`#design/button.html:icon-only~storybook`.

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
before reading Storybook's index.

- Use `ready` with the `/index.json` URL. Storybook listens on its port before
  it has finished building, and the index is what Workbench reads.
- Raise `timeout` for large Storybooks. The maximum is 300 seconds.
- Pass `--no-open` (or `--ci`) so Storybook doesn't open a browser tab.
- If startup times out, the canvas still opens with your pages, and their
  Storybook lenses report the problem when you open them.

Start commands run only in trusted workspaces, and only in VS Code. See
[Start commands](configuration.md#start-commands) for every key.

## Switching stories

The first story loads in an iframe at
`iframe.html?id=<story-id>&viewMode=story`, through the loopback proxy
Workbench runs for each implementation (see
[Embedding and sign-in](lenses.md#embedding-and-sign-in)). After that, Workbench
switches stories through Storybook's own channel, so the preview stays loaded
and switching is fast. If Storybook doesn't confirm a switch within about
1.5 seconds, or doesn't finish rendering within 15 seconds, Workbench loads the
story's URL instead.

On single-view pages, switching to Docs, Preview, or another implementation lens
keeps the Storybook preview loaded. Returning preserves edits and opened menus
while it is kept. **Reload** starts the selected preview afresh.
Opening a page with multiple artboards also keeps the Storybook preview loaded
for up to 15 minutes. Returning shows it again without loading Storybook's
preview again.

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

A [design-system export](design-system-export.md) includes your Storybook
configuration from `.storybook`, or a `--config-dir` named in a package script,
when the exported sources need it. Exported through the Storybook lens, it
captures a reference screenshot of every story of each mapped page at each of
the page's `sizes`. Set `root` so sources can be found.

## Troubleshooting

| Symptom | Check |
| --- | --- |
| No Storybook lens on a page | The title is exact. Ask the [stories route](troubleshooting.md#check-a-storybook-title), which lists near matches. |
| `url: auto` finds nothing | Storybook isn't on a scripted port or 6006 to 6010. Set `url` explicitly, or in `workbench.local.yaml`. |
| Lens is blank or shows an error | Without `url`: read the error on the canvas, which includes Storybook's output, and check that `storybook` is installed in `root` and the workspace is trusted. With `url`: check that Storybook is running there: open `<url>/index.json` in a browser. Then open `<url>/iframe.html` directly, and check the frame's console. |
| No source links | `root` is missing or points at the wrong folder. It should be the folder Storybook runs from. |
| Screenshots don't match what you see | The story hadn't finished loading, so the bridge hadn't connected. Take the screenshot again. |
