# Getting started

This guide takes you from installing the extension to a working canvas with
two screens and a page state. It takes about five minutes.

## 1. Install the extension

Download the `.vsix` for your computer from the
[latest Workbench release](https://github.com/canonic2/canonic/releases/latest):

| Computer | File |
| --- | --- |
| Mac with Apple silicon | `canonic-workbench-darwin-arm64.vsix` |
| Mac with Intel | `canonic-workbench-darwin-x64.vsix` |
| Windows | `canonic-workbench-win32-x64.vsix` |
| Windows on Arm | `canonic-workbench-win32-arm64.vsix` |
| Linux | `canonic-workbench-linux-x64.vsix` |
| Linux on Arm | `canonic-workbench-linux-arm64.vsix` |

Install it from the terminal:

```sh
code --install-extension canonic-workbench-darwin-arm64.vsix
```

Or open the Extensions view in VS Code, choose **…** › **Install from VSIX…**, and pick the
file. Cursor, Windsurf, and other VS Code forks accept the same file through their own
command or menu.

Each file bundles its platform's screenshot helper, so there is nothing else to
install. VS Code 1.75 or later is required.

> The release builds are unsigned. Desktop macOS is the most exercised
> platform; Windows and Linux builds have not been validated as thoroughly.

If VS Code was already open on a project, run **Developer: Reload Window**
after installing or updating, so running windows pick up the new version.

## 2. Write `workbench.yaml`

Create `workbench.yaml` at the root of your project. Every `src` is a path from
that root:

```yaml
name: Acme

sections:
  - name: Pages
    icon: file-text
    items:
      - label: Sign in
        src: pages/sign-in.html
        states:
          - id: default
            label: Default
          - id: error
            label: Wrong password

  - name: Components
    icon: component
    items:
      - label: Button
        src: preview/button.html
```

- `name` titles the screen list and the canvas tab.
- Each **section** becomes a button in the sidebar's section list. `icon` is any
  [Lucide](https://lucide.dev/icons/) icon name, written in kebab-case.
- Each **item** is a screen: a `label` and the HTML file to show.
- `states` lists variations of one page. The first is the page as written.

The [configuration reference](configuration.md) lists every key.

## 3. Make the page answer to its state

Declaring a state in the YAML adds a row to the sidebar. The page decides what
that state looks like. The simplest way is CSS keyed off an attribute the
workbench sets on `<html>`:

```html
<!-- pages/sign-in.html -->
<style>
  .alert { display: none; }
  html[data-wb-state="error"] .alert { display: block; }
</style>

<p class="alert">That password isn't right.</p>
```

There are two other ways, covered in [Pages and states](pages-and-states.md).
You don't add any script tags: the extension injects what a page needs when it
serves it.

## 4. Open the canvas

Open the project folder in VS Code. When a folder contains `workbench.yaml`, the
**Workbench** icon appears in the activity bar. Its view shows your sections and
screens where a file tree usually goes.

Pick a screen, or run **Workbench: Open Canvas** from the Command Palette. The
canvas opens in an editor tab and shows the screen at a real device width.

Try these:

- Pick **Wrong password** under **Sign in** to switch state.
- Use the width buttons in the toolbar for desktop, mobile, a resizable frame,
  or **Fit**.
- Draw on the screen with the markup tools at the bottom, then select
  **Copy handoff**. The annotated screenshot is saved and a prompt describing
  every mark is copied to your clipboard, ready to paste to an agent.

In VS Code, saving `workbench.yaml` or `workbench.local.yaml` rebuilds the screen
list and refreshes the canvas. **Workbench: Refresh Screens** does the same on
demand. In a standalone browser, reload the page.

## 5. Ignore the files that belong to one machine

Add these to `.gitignore`:

```gitignore
workbench.local.yaml
.canonic/.handoffs/
```

- `workbench.local.yaml` holds overrides for your machine, such as a dev
  server's port or where another repository is checked out. See
  [Local overrides](configuration.md#local-overrides).
- `.canonic/.handoffs/` holds the screenshots that handoffs save.

## Removing Workbench

Delete `workbench.yaml`. Your pages contain nothing Workbench-specific beyond
optional state attributes and CSS, which are inert without it.

## Next steps

- Give pages more states and choose their viewports:
  [Pages and states](pages-and-states.md).
- Show the same screen as it runs on your dev server:
  [Lenses and URL implementations](lenses.md).
- Use your Storybook: [Storybook](storybook.md).
