# React Native Web adapter

Part of the [adapters](README.md). User guide:
[React Native Web](../../docs/react-native-web.md).

## Source

A preview's `source` is a React Native component; `inputs` are its props. A
docs example of a `react-native-web` lens is a component too. React,
React DOM and `react-native-web` come from the project.

## Build

The [React adapter](react.md)'s compilation, with the `react-native-web`
profile's resolution
([compiler](../preview-pipeline/compiler.md#one-compiler-per-project-and-profile)):

- `react-native` resolves to `react-native-web`, as an exact alias;
- `.web.tsx`, `.web.ts`, `.web.jsx` and `.web.js` are tried before the
  shared extensions, unless the project sets `resolveExtensions`;
- [react-native-web.css](../../ui/preview/react-native-web.css), the host's
  styles, is imported by each entry and kept as a side effect.

The profile has its own compiler, so its output never mixes with the `web`
profile's.

## Project code on the server

None.

## Browser

`mountNative` in [react.js](../../ui/preview/react.js) adds the
`wb-react-native-root` class to the canvas, a column flex layout, so a root
view with `flex: 1` fills the frame at fixed and resizable sizes without
project CSS, then mounts as the React adapter does. Cleanup removes the
class it added.

## Edits

As the React adapter.

## Target

Required for the Rust server ([decisions](README.md#decisions)). Every part
of the profile is an Rspack resolve or rule option in its Rust crates (an
alias, extensions, a side-effect rule), with the React adapter's SWC
loader. No JavaScript loader is involved.

## Status

Implemented in the Node compile worker (`options.ts`, `project.ts`). Not
implemented in Rust.

## Open questions

- **Untranspiled packages.** Some React Native libraries publish Flow or
  untranspiled JSX in `node_modules`, which Metro compiles with Babel. The
  current profile does not compile `node_modules` either; whether the Rust
  server should, and with what, is open.
