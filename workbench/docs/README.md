# Workbench documentation

Workbench is a VS Code extension that puts every screen in your repository on
one canvas, at real device widths. You can draw on a screen, compare it with
its implementation, and hand an annotated screenshot to your coding agent.

A project's whole setup is one file, `workbench.yaml`, at the project root.
Nothing is bundled into your pages, and nothing is copied into your repository.

## Start here

| Guide | Read it to |
| --- | --- |
| [Getting started](getting-started.md) | Install the extension, write a first `workbench.yaml`, and open the canvas |
| [Pages and states](pages-and-states.md) | Write design pages, give one page several states, and choose its viewports |
| [Using the canvas](canvas.md) | Find your way around the sidebar, the toolbar, frame widths, zoom, and links |
| [Markup and handoff](markup-and-handoff.md) | Annotate a screen, save screenshots, and hand the result to an agent |

## Connect your implementation

| Guide | Read it to |
| --- | --- |
| [Lenses and URL implementations](lenses.md) | Show a screen as it runs on a dev server or staging, point at its code, and start the server automatically |
| [Storybook](storybook.md) | Map screens to stories, or import a whole Storybook as the workbench |
| [iOS Simulator](ios-simulator.md) | Stream and drive a booted Simulator on the canvas |
| [App windows](windows.md) | Stream a window from any macOS app, such as an Android emulator |

## Reference

| Guide | Contents |
| --- | --- |
| [workbench.yaml reference](configuration.md) | Every key, its type, its default, and the rules the reader enforces |
| [Design-system export](design-system-export.md) | What the ZIP export contains and how to use it |
| [The VS Code extension](extension.md) | Commands, settings, the server and its ports, screenshots, logs, and running without the editor |
| [Troubleshooting](troubleshooting.md) | How to find out why a screen, lens, story, or screenshot is missing or wrong |

## For contributors

The [specifications](specs/README.md) record the observable contracts the
implementation is tested against. The [workbench README](../workbench/README.md)
describes the browser code file by file, and the
[extension README](../README.md#working-on-it) covers building, packaging, and
releasing.
