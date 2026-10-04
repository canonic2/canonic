# React Native Web previews

The `react-native-web` adapter renders a React Native component in the
browser through [React Native Web](https://necolas.github.io/react-native-web/),
with the preview's inputs as its props. Use it to review shared components and
screens at real device widths, next to their designs, without building the app.

It shows the web rendering, which can differ from iOS and Android. To review
the native app itself, stream it from the [iOS Simulator](ios-simulator.md) or
an [app window](windows.md), such as an Android emulator.

The adapter works like the [React adapter](react.md), with two differences:
imports of `react-native` load `react-native-web`, and `.web.*` files are
preferred. Inputs, callbacks, providers, styles, aliases, and `dedupe` work as
described in that guide. The definition keys, states, controls, and hooks are
described in [TypeScript Workbench previews](workbench-previews.md).

## Requirements

- `react-native-web`, `react`, and `react-dom` 18 or later, installed in your
  project. The `react-native` package isn't used by previews and doesn't need
  to be installed for them. Workbench finds these packages from the
  definition's folder and never installs or replaces them.
- `workbench.yaml` at the project root. `name: Acme` is enough.
- A [trusted workspace](extension.md#workspace-trust), since previews run
  project code.

## Write a first preview

```text
acme-app/
├── workbench.yaml
├── package.json             react, react-dom, react-native-web
└── src/
    ├── assets/
    │   ├── avatar.png
    │   └── acme-sans.woff2
    ├── components/
    │   ├── ProfileCard.tsx
    │   └── ProfileCard.workbench.ts
    ├── haptics.ts           the native implementation
    ├── haptics.web.ts       the web implementation, used by previews
    └── preview/
        ├── canvas.css
        └── environment.tsx
```

`src/components/ProfileCard.tsx`:

```tsx
import { Image, Platform, Pressable, StyleSheet, Text, View } from 'react-native';
import { tap } from '../haptics';
import avatar from '../assets/avatar.png';

export interface ProfileCardProps {
  name: string;
  role: string;
  online?: boolean;
  onPress?: () => void;
}

export function ProfileCard({ name, role, online = false, onPress }: ProfileCardProps) {
  return (
    <Pressable
      accessibilityRole="button"
      onPress={() => { tap(); onPress?.(); }}
      style={styles.card}
    >
      <Image source={{ uri: avatar }} style={styles.avatar} />
      <View>
        <Text style={styles.name}>{name}</Text>
        <Text style={styles.role}>{role} · {Platform.OS}</Text>
      </View>
      {online && <View style={styles.dot} />}
    </Pressable>
  );
}

const styles = StyleSheet.create({
  card: { flexDirection: 'row', alignItems: 'center', gap: 12, padding: 16, borderRadius: 12, backgroundColor: '#ffffff' },
  avatar: { width: 48, height: 48, borderRadius: 24 },
  name: { fontFamily: 'Acme Sans', fontSize: 17, fontWeight: '600', color: '#111827' },
  role: { fontSize: 14, color: '#6b7280' },
  dot: { width: 10, height: 10, borderRadius: 5, backgroundColor: '#16a34a' },
});
```

`src/haptics.ts` uses a native module, and `src/haptics.web.ts` replaces it in
the browser:

```ts
// src/haptics.ts
import Haptics from 'react-native-haptic-feedback';

export function tap() {
  Haptics.trigger('impactLight');
}
```

```ts
// src/haptics.web.ts
export function tap() {
  // Browsers have no haptic feedback.
}
```

`src/components/ProfileCard.workbench.ts`:

```ts
import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'components/profile-card',
  title: 'Components/Profile Card',
  adapter: 'react-native-web',
  source: { entry: './ProfileCard.tsx', export: 'ProfileCard' },
  environment: '../preview/environment.tsx',
  styles: ['../preview/canvas.css'],
  viewports: ['mobile', 'fit'],
  inputs: { name: 'Avery Example', role: 'Designer', online: false },
  controls: {
    name: { type: 'text' },
    role: { type: 'text' },
    online: { type: 'boolean' },
  },
  states: {
    offline: {},
    online: { inputs: { online: true } },
  },
});
```

`src/preview/environment.tsx` fills the frame and reports presses under
**Actions** in **Preview controls**:

```tsx
import { cloneElement, type ReactElement } from 'react';
import { View } from 'react-native';
import type { PreviewContext } from '@canonic2/workbench';

export function wrap(element: ReactElement<Record<string, unknown>>, context: PreviewContext) {
  return (
    <View style={{ flex: 1, padding: 16, backgroundColor: '#f3f4f6' }}>
      {cloneElement(element, { onPress: () => context.action('press') })}
    </View>
  );
}
```

`src/preview/canvas.css` lets `flex: 1` fill the frame and declares the font:

```css
#workbench-preview {
  display: flex;
  flex-direction: column;
}

@font-face {
  font-family: 'Acme Sans';
  src: url(../assets/acme-sans.woff2) format('woff2');
}
```

Run **Workbench: Refresh Screens**. **Profile Card** appears in a
**Components** section with the states **Offline** and **Online**, and
`Platform.OS` is `web`.

Keep the definition free of component imports: Workbench runs it in Node to
list previews. Import only types from component files.

## How imports resolve

- An import of exactly `react-native` loads `react-native-web`. Deep imports
  such as `react-native/Libraries/...` aren't rewritten and fail with
  `Could not resolve`. Give each one an alias, or move the code into a
  `.web.*` file.
- For an import without an extension, Workbench tries `.web.tsx`, `.web.ts`,
  `.web.jsx`, and `.web.js` first, then `.tsx`, `.ts`, `.jsx`, `.js`, `.mjs`,
  `.cjs`, `.json`, `.vue`, and `.html`. Files ending in `.ios.*`,
  `.android.*`, or `.native.*` are never chosen.
- Setting `resolveExtensions` in `workbench.config.ts` replaces the whole list,
  including the `.web.*` entries. Include them if you set it.
- An alias for `react-native` in `workbench.config.ts` replaces the built-in
  one, if you need your own shim.

## Data and providers

Providers, inputs, and data work as they do for [React](react.md#add-providers-and-context).
A screen that loads its data over the network keeps doing so: answer its
`fetch` and XMLHttpRequest calls per state with
[`requests`](preview-data.md#request-mocks), or wrap it in a provider
holding fixture data from an environment.

## Replace native-only modules

A module that needs native code can't run in the browser. Replace it in one of
two ways:

- **A `.web.*` file** next to your own module, as `haptics.web.ts` above. This
  also serves a web build of the app, if you have one.
- **An alias** in `workbench.config.ts` that points the package at a mock. The
  mock affects previews only:

  ```ts
  import { defineConfig } from '@canonic2/workbench';

  export default defineConfig({
    aliases: { 'react-native-haptic-feedback': './src/preview/haptics-mock.ts' },
  });
  ```

  ```ts
  // src/preview/haptics-mock.ts
  export default { trigger(_type: string) {} };
  ```

Aliases match whole import specifiers, and relative alias paths start from the
project root. See
[Imports, JSX, and environment variables](react.md#imports-jsx-and-environment-variables).

## Layout and full-height screens

The component renders inside an element with the ID `workbench-preview`, which
is at least as tall as the frame but isn't a flex container. A root view with
`flex: 1` therefore takes only the height of its content. To fill the frame,
add the `#workbench-preview` rule above in a stylesheet listed in `styles`, as
in the example, or give the root view a height.

Choose the frame sizes with `viewports`; `mobile` suits most screens. See
[Viewports](pages-and-states.md#viewports).

## Images and fonts

- Importing an image, with `import avatar from './avatar.png'` or
  `require('./avatar.png')`, gives its URL. `Image` accepts it as
  `source={{ uri: avatar }}` or `source={avatar}`.
- Workbench loads the file you name. It doesn't pick `@2x` or `@3x` variants.
- Declare custom fonts with `@font-face` in a stylesheet listed in `styles`,
  then use the family name in `fontFamily`. Font files can be `.woff`,
  `.woff2`, or `.ttf`.

The other file types, and what needs a compiler plugin, are listed in
[Styles, fonts, and images](react.md#styles-fonts-and-images).

## Packages that need extra settings

Some React Native packages assume Metro, React Native's bundler. Settings in
`workbench.config.ts` cover the common cases:

- **`__DEV__` or `global`** fail with `__DEV__ is not defined` or
  `global is not defined`. Define them.
- **JSX in `.js` files** fails with
  `The JSX syntax extension is not currently enabled`. Add a compiler plugin
  that loads that package's `.js` files as JSX.
- **Flow type annotations** fail with a syntax error, such as
  `Expected ")" but found ":"`. They need a compiler plugin that strips Flow
  types, or an alias to a compiled build of the package.

```ts
import fs from 'node:fs';
import { defineConfig } from '@canonic2/workbench';

// acme-native-lib publishes JSX in .js files.
const jsxInJs = {
  name: 'jsx-in-js',
  setup(build: any) {
    build.onLoad({ filter: /node_modules[\\/]acme-native-lib[\\/].*\.js$/ }, async (args: { path: string }) => ({
      contents: await fs.promises.readFile(args.path, 'utf8'),
      loader: 'jsx',
    }));
  },
};

export default defineConfig({
  define: { __DEV__: 'true', global: 'globalThis' },
  plugins: [jsxInJs],
});
```

Compiler plugins use [esbuild's plugin interface](https://esbuild.github.io/plugins/).
See [the configuration file](custom-adapters.md#configuration-file).

## Errors and fixes

| Error | Cause and fix |
| --- | --- |
| `Could not resolve "react-native-web"`, `"react"`, or `"react-dom/client"` | A required package isn't installed where the definition can find it. Install it in the project. |
| `Could not resolve "<native package>"` | A native-only module. [Replace it](#replace-native-only-modules) with a `.web.*` file or an alias. |
| `Could not resolve "react-native/…"` | A deep import into `react-native`. [Alias it or move it into a `.web.*` file](#how-imports-resolve). |
| `__DEV__ is not defined`, `global is not defined` | Add them to `define`. See [Packages that need extra settings](#packages-that-need-extra-settings). |
| `The JSX syntax extension is not currently enabled` | A package publishes JSX in `.js` files. Add a [plugin](#packages-that-need-extra-settings). |
| A full-screen view is only as tall as its content | The preview root isn't a flex container. See [Layout and full-height screens](#layout-and-full-height-screens). |

The errors shared with the React adapter, such as duplicate React copies,
missing exports, and environment variables, are listed in
[React previews](react.md#errors-and-fixes).
