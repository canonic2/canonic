# Workbench documentation

Workbench is a VS Code extension that renders a project's pages on one
canvas at device widths. You can draw on a page, compare it with its
implementation, and copy a screenshot with your annotations and a description
of them for a coding agent.

A project's configuration is `workbench.yaml` at the project root. Pages can
come from HTML design pages you list there, from Markdown pages with live
examples, from TypeScript previews of your own components, or from a
Storybook or Simulator catalog, and any page can have Markdown docs as a
lens. The tool ships inside
the extension; in your project it writes only handoff screenshots and the
collections you edit with **Configure pages**. See
[Files and network access](extension.md#files-and-network-access).

## Start here

| Guide | Read it to |
| --- | --- |
| [Getting started](getting-started.md) | Install the extension, write a first `workbench.yaml`, and open the canvas |
| [Pages and states](pages-and-states.md) | Write design pages, give one page several states, and choose its sizes |
| [Docs](docs-pages.md) | Document components and foundations in Markdown, read as a lens beside a page's design, with live examples and their code |
| [Using the canvas](canvas.md) | Find your way around the sidebar, the top bar, artboard sizes, zoom, and links |
| [Annotations and handoff](annotations-and-handoff.md) | Annotate a page with the toolbar, save screenshots, and hand the result to an agent |
| [Several spaces](spaces.md) | Switch between spaces in one window, add spaces from elsewhere on disk, and serve several from the command line |

## Previews

| Guide | Read it to |
| --- | --- |
| [TypeScript Workbench previews](workbench-previews.md) | Define a preview, its states, and its hooks; check previews from the command line and build a portable viewer |
| [Preview data, mocks, and actions](preview-data.md) | Give a page its props, providers, and data; mock its requests; log its actions; and link previews into flows |
| [React](react.md) | Preview React components, with providers, styles, and assets |
| [React Native Web](react-native-web.md) | Preview React Native components in the browser, with web-only files and mocks for native modules |
| [Vue](vue.md) | Preview Vue single-file components, with plugins, slots, and events |
| [Astro](astro.md) | Preview Astro components and pages rendered on the server |
| [HTML](html.md) | Preview plain HTML, CSS, and scripts, or mount anything from a module |
| [Custom adapters](custom-adapters.md) | Register another technology, compiler plugins, aliases, and build constants |

## Connect your implementation

| Guide | Read it to |
| --- | --- |
| [Lenses and URL implementations](lenses.md) | Compare a design with previews or a dev, staging, or live website; customize every lens label, point at its code, and start the server automatically |
| [Storybook](storybook.md) | Map pages to stories, or import a whole Storybook as the workbench |
| [iOS Simulator](ios-simulator.md) | Stream and drive a booted Simulator on the canvas |
| [App windows](windows.md) | Stream a window from any macOS app, such as an Android emulator |

## Reference

| Guide | Contents |
| --- | --- |
| [workbench.yaml reference](configuration.md) | Every key, its type, its default, and the rules the reader enforces |
| [Design-system export](design-system-export.md) | What the ZIP export contains, including portable previews, and how to use it |
| [The VS Code extension](extension.md) | Commands, projects in subfolders, the server and its ports, the preview worker, screenshots, files and network access, updating, uninstalling, and running without the editor |
| [Troubleshooting](troubleshooting.md) | How to find out why a page, lens, story, or screenshot is missing or wrong |
