# Getting started

This guide takes you from installing the extension to a working canvas with
design pages, a page state, and a preview of a component from your code. It
takes about five minutes.

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
file. Cursor, Windsurf, and other VS Code forks accept the same file, from the same
Extensions view menu or from their own command-line tool, such as
`cursor --install-extension`.

Each file bundles the runtime its platform needs for screenshots and
TypeScript previews, so there is nothing else to install. VS Code 1.75 or later
is required. Screenshots need macOS 13 or later, Windows, or Linux with a
display; see [The screenshot helper](extension.md#the-screenshot-helper).

> The release builds are unsigned. Desktop macOS is the most exercised
> platform; Windows and Linux builds have not been validated as thoroughly.

If VS Code was already open on a project, run **Developer: Reload Window**
after installing, so running windows pick up the extension. To update or remove
it later, see [Updating](extension.md#updating) and
[Uninstalling](extension.md#uninstalling).

## 2. Write `workbench.yaml`

Create `workbench.yaml` at the root of your project. Every `src` is a path from
that root. Workbench looks for the file at the top of each folder open in
VS Code, so in a monorepo either put it at the repository root, or open the
folder that contains it; see [Projects in a subfolder](extension.md#projects-in-a-subfolder).

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
        src: components/button.html
```

- `name` titles the screen list in a standalone browser and names
  [design-system exports](design-system-export.md).
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
- Use the width buttons in the toolbar for **Laptop**, **Mobile**, a
  **Resizable** frame, or **Fit**.
- Draw on the screen with the markup tools at the bottom, then select
  **Copy handoff**. The annotated screenshot is saved and a prompt describing
  every mark is copied to your clipboard, ready to paste to an agent.

Workbench also lists any [TypeScript previews](workbench-previews.md) it
finds in `*.workbench.ts` and `*.workbench.tsx` files, once you trust the
workspace. Step 6 adds one.

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

Workbench writes nothing else into the project unless you save
**Configure pages**, which rewrites `workbench.yaml`. See
[Files and network access](extension.md#files-and-network-access).

## 6. Preview a component from your code

Design pages show what a screen should look like. A TypeScript preview renders
the real component from your source, in named states. Next to a React
`src/Button.tsx` that exports `Button`, add `src/Button.workbench.ts`:

```ts
import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'components/button',
  title: 'Components/Button',
  adapter: 'react',
  source: { entry: './Button.tsx', export: 'Button' },
  inputs: { label: 'Continue', disabled: false },
  states: {
    default: {},
    disabled: { inputs: { disabled: true } },
  },
});
```

Run **Workbench: Refresh Screens**. A second **Button** appears under
**Components**, with **Default** and **Disabled** states, rendered with your
project's own React. Workbench supplies `@canonic2/workbench` when it compiles
the file; to type-check it, run `npm install --save-dev @canonic2/workbench`
(see [Types](workbench-previews.md#types)). Other frameworks work
the same way; each has its own guide: [React](react.md),
[React Native Web](react-native-web.md), [Vue](vue.md), [HTML](html.md), and
[Astro](astro.md). For anything else, see [Custom adapters](custom-adapters.md).

## Removing Workbench from a project

Delete `workbench.yaml` and `workbench.local.yaml`, along with any
`.workbench.ts` and `.workbench.tsx` files, `workbench.config.ts`, and
`workbench-env.d.ts` you added, and the `.canonic/.handoffs/` folder. Your
pages contain nothing Workbench-specific beyond optional state attributes and
CSS, which are inert without it. To remove the extension itself, see
[Uninstalling](extension.md#uninstalling).

## Next steps

- Add controls, hooks, and other frameworks to previews:
  [TypeScript Workbench previews](workbench-previews.md).
- Learn the canvas's controls and shortcuts: [Using the canvas](canvas.md).
- Hand a marked-up screen to your agent: [Markup and handoff](markup-and-handoff.md).
- Give pages more states and choose their viewports:
  [Pages and states](pages-and-states.md).
- Show the same screen as it runs on your dev server:
  [Lenses and URL implementations](lenses.md).
- Use your Storybook: [Storybook](storybook.md).
