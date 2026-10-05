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
  sizes: ['mobile', 'fit'],
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

`src/preview/canvas.css` declares the font. A stylesheet is only needed when
the preview uses custom fonts or other project CSS:

```css
@font-face {
  font-family: 'Acme Sans';
  src: url(../assets/acme-sans.woff2) format('woff2');
}
```

Run **Workbench: Refresh Pages**. **Profile Card** appears in a
**Components** collection with the states **Offline** and **Online**, and
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
A component that loads its data over the network keeps doing so: answer its
`fetch` and XMLHttpRequest calls per state with
[`requests`](preview-data.md#request-mocks), or wrap it in a provider
holding fixture data from an environment.

## Preview a screen inside its app layout

A preview mounts the component named in `source`. Pointing it at a screen
doesn't mount the app's parent layouts. In an Expo Router app, a route such as
`app/(tabs)/index.tsx` therefore renders without the tabs from
`app/(tabs)/_layout.tsx`, the root navigation theme, or the app's other
providers. A full-height preview supplies space; the project supplies the
screen's surroundings.

Use an [environment](preview-data.md#environments) to wrap screen previews in
the same providers, background, header, and tab bar as the app. Keep isolated
component previews in a separate environment so a button or card doesn't get
the app's navigation around it.

Prefer sharing the app's shell components and navigation options. If the
production layout depends on file-based routing, extract the reusable shell
or create a preview wrapper with an in-memory navigator. Mount the previewed
screen as the active route's content. This retains the navigator's scene
sizing and tab-bar space instead of drawing a tab bar over the screen.
Keep any preview-specific composition in step with the production layout.

For example, suppose your project has `AppProviders` for its theme and session,
and a `ScreenPreviewShell` that renders children inside the app's tab layout.
These are project components, not Workbench APIs. The shell accepts `activeTab`
and calls `onTabPress` with `'home'` or `'explore'`:

```tsx
// src/preview/screen-environment.tsx
import type { ReactElement } from 'react';
import type { PreviewContext } from '@canonic2/workbench';
import { AppProviders } from '../app/AppProviders';
import { ScreenPreviewShell } from './ScreenPreviewShell';

export function wrap(element: ReactElement, context: PreviewContext) {
  const activeTab = context.globals.activeTab === 'explore' ? 'explore' : 'home';
  return (
    <AppProviders>
      <ScreenPreviewShell
        activeTab={activeTab}
        onTabPress={tab => context.navigate('mobile-app/' + tab)}
      >
        {element}
      </ScreenPreviewShell>
    </AppProviders>
  );
}
```

Wire that environment into each tab screen's definition and map its tab
addresses to previews:

```ts
// src/preview/home.workbench.ts
import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'mobile-app/home',
  title: 'Mobile App/Home',
  adapter: 'react-native-web',
  source: { entry: '../screens/HomeScreen.tsx' },
  environment: './screen-environment.tsx',
  sizes: ['mobile', 'resizable'],
  globals: { activeTab: 'home' },
  links: { '/home': 'mobile-app/home', '/explore': 'mobile-app/explore' },
});
```

Create the Explore definition with its own source, `id: 'mobile-app/explore'`,
and `globals: { activeTab: 'explore' }`. Reuse the environment and link map.
The callback navigates by preview ID; `links` handles anchors using those
addresses. Navigation runs while **Actions** is on. See
[Links and navigation](preview-data.md#links-and-navigation). Alternatively, keep
the other tabs decorative and report presses with `context.action('tab', tab)`
so reviewing a state doesn't leave that screen.

Use a separate shell for modal or stack screens: their header, background,
and available content area may differ from a tab screen. Providers shared by
all React Native Web previews can live in an adapter-scoped project environment;
each preview's environment then supplies its own shell. The project environment
wraps the preview environment on the outside. See
[Environments](preview-data.md#environments).

Keep `.workbench.ts` files and preview wrappers outside Expo Router's route
directory. Files in that directory are interpreted as routes. A router mock
that implements links can support isolated screens, but doesn't supply layouts
or navigation context; screens that use navigation hooks need a compatible
navigator or a mock that implements those hooks.

### Safe areas and device dimensions

Choose the same logical width and height as the device you are comparing with,
using the [Resizable artboard](canvas.md#artboard-sizes) when needed. A Simulator image's
pixel dimensions can be larger than the device's logical dimensions. The
`mobile` size is a preset, not a simulation of every phone model.

If the app uses `react-native-safe-area-context`, supply its contexts in the
preview environment. A desktop browser usually reports zero phone insets.
`SafeAreaProvider initialMetrics` seeds the first render; its web implementation
then measures browser insets, so initial values alone don't hold simulated
phone insets throughout the preview.

For deterministic browser previews, a project wrapper can provide the frame
and inset contexts directly. This example requires
`react-native-safe-area-context` in the project. The inset values are illustrative;
choose values for the device and orientation being reviewed:

```tsx
// src/preview/PhoneMetrics.tsx
import type { PropsWithChildren } from 'react';
import { useWindowDimensions } from 'react-native';
import { SafeAreaFrameContext, SafeAreaInsetsContext } from 'react-native-safe-area-context';

const insets = { top: 59, right: 0, bottom: 34, left: 0 };

export function PhoneMetrics({ children }: PropsWithChildren) {
  const { width, height } = useWindowDimensions();
  return (
    <SafeAreaFrameContext.Provider value={{ x: 0, y: 0, width, height }}>
      <SafeAreaInsetsContext.Provider value={insets}>
        {children}
      </SafeAreaInsetsContext.Provider>
    </SafeAreaFrameContext.Provider>
  );
}
```

Place `PhoneMetrics` outside the shell that consumes it. Avoid nesting another
`SafeAreaProvider` that replaces those simulated values. Supplying insets
doesn't add padding: the app's `SafeAreaView`, navigator, or layout must consume
them. Preserve the app's ownership of that spacing so the preview doesn't
apply the same inset twice. This wrapper doesn't draw a status bar, notch, or
home indicator.

A standalone preview's window is its iframe, so `useWindowDimensions` measures
that frame as it resizes. A docs example lives in a panel inside a larger
document; a phone surface embedded there needs its own measurements rather
than assuming the document's window is the panel size.

### Compare browser and native rendering

Even with the app shell, React Native Web renders the web platform:

- `Platform.OS` is `'web'`, so `Platform.select` can choose different copy or
  behavior. An iOS branch isn't selected by choosing a phone-sized frame.
- Fonts, text wrapping, scrolling, and native controls can differ. Load the
  project's web fonts and compare at the same logical size and theme.
- Native menus, keyboards, system bars, and gestures need the native runtime.

Use the [iOS Simulator lens](ios-simulator.md) or an
[Android emulator window](windows.md) to review those differences alongside
the browser preview. Check the native screen itself for missing safe-area
handling before adding preview-only padding to make it look correct.

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
is at least as tall as the frame and uses a column flex layout. A root view with
`flex: 1` fills the frame without a stylesheet, including when the frame is
resized. Project CSS can override the host's default layout.

Choose the artboard sizes with `sizes`; `mobile` suits most app screens. See
[Sizes](pages-and-states.md#sizes).

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

## Document components with a docs page

A page's [docs](docs-pages.md) can show React Native components as live
examples. Give the page a `docs` lens with `adapter: react-native-web`. Each
example exports a component: the default export of each file in a folder, or
each named export of one file. Imports resolve as they do for previews, with
`react-native` loading `react-native-web` and `.web.*` files preferred. Example
file names must be kebab-case, so `online.web.tsx` isn't an example; put the
`.web.*` file next to the module the example imports instead.

When the component also has a React DOM implementation for the web, give the
page a lens for each. Both show the same Markdown, and the lens switcher in
the top bar changes what renders the examples:

```yaml
implementations:
  web:
    kind: docs
    label: Web
    adapter: react
  native:
    kind: docs
    label: React Native Web
    adapter: react-native-web
    environment: src/preview/environment.tsx
    styles:
      - src/preview/canvas.css

collections:
  - name: Components
    items:
      - label: Profile Card
        src: docs/profile-card.md
        lens: native
        implementations:
          web: web/src/profile-card/examples/
          native: src/components/profile-card-examples/
```

`src/components/profile-card-examples/online.tsx`:

```tsx
import { ProfileCard } from '../ProfileCard';

export default function Online() {
  return <ProfileCard name="Avery Example" role="Designer" online />;
}
```

The web lens's folder has its own `online.tsx` that renders the web
component. An example one lens has and the other doesn't shows
*Not available in* and the lens's label.

Examples get no inputs or controls, so set the props in each example. The
lens's `environment` wraps each example as it wraps a preview, and its
`styles` load with the examples, here for the `@font-face` rule. The
adapter supplies a column flex host in each panel, so a root view with
`flex: 1` fills the panel's available content area. Panels keep their own
padding and content-based height; they don't become full-screen frames.

## Errors and fixes

| Error | Cause and fix |
| --- | --- |
| `Could not resolve "react-native-web"`, `"react"`, or `"react-dom/client"` | A required package isn't installed where the definition can find it. Install it in the project. |
| `Could not resolve "<native package>"` | A native-only module. [Replace it](#replace-native-only-modules) with a `.web.*` file or an alias. |
| `Could not resolve "react-native/…"` | A deep import into `react-native`. [Alias it or move it into a `.web.*` file](#how-imports-resolve). |
| `__DEV__ is not defined`, `global is not defined` | Add them to `define`. See [Packages that need extra settings](#packages-that-need-extra-settings). |
| `The JSX syntax extension is not currently enabled` | A package publishes JSX in `.js` files. Add a [plugin](#packages-that-need-extra-settings). |
| A full-screen view is only as tall as its content | Check that the root view and any environment wrappers grow with `flex: 1`. See [Layout and full-height screens](#layout-and-full-height-screens). |
| Tabs, a header, or the app background are missing | The preview mounts the screen without its parent layout. Add a [screen environment](#preview-a-screen-inside-its-app-layout). |
| Phone safe-area spacing is missing or changes after mount | Supply and consume deterministic metrics in the preview wrapper. See [Safe areas and device dimensions](#safe-areas-and-device-dimensions). |

The errors shared with the React adapter, such as duplicate React copies,
missing exports, and environment variables, are listed in
[React previews](react.md#errors-and-fixes).
