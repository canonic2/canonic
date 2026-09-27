# Authored pages and other implementations

This spec covers previews other than Storybook: project HTML pages, URL lenses,
and iOS Simulator lenses. They share the selection and frame behavior in
[core.md](core.md).

## Authored HTML pages

```yaml
sections:
  - name: Pages
    items:
      - label: Sign in
        src: pages/sign-in.html
        states:
          - id: default
            label: Default
          - id: error
            label: Wrong password
```

- `src` is relative to the project root and cannot start with `/` or contain
  `..`, `:`, or `~`. An authored screen's first state is the page as authored;
  the address omits it. Other state IDs must be kebab-case.
- The server injects `states.js` and `actions.js` into pages it serves. A
  selected nondefault state travels as `?state=<id>`. `states.js` sets
  `data-wb-state` on `<html>` and applies `data-wb-state-only`,
  `data-wb-state-not`, and `data-wb-set-<id>` declarations. Declaring a state
  in YAML alone does not make the page render it.
- Actions are off by default in the workbench: links do not navigate and
  forms do not submit. Turning actions on lets a page behave normally.
  Navigation to another configured local HTML screen is handed to the
  workbench, which keeps the sidebar selection in step. An authored page
  opened outside the served workbench has normal link behavior.

## URL implementations

```yaml
implementations:
  staging:
    kind: url
    base: https://staging.example.com
  dev:
    kind: url
    base: http://localhost:3710

sections:
  - name: Pages
    items:
      - label: Sign in
        src: pages/sign-in.html
        implementations:
          staging: /sign-in
          dev:
            default: /sign-in
            error: /sign-in?error=1
```

- A URL implementation requires an HTTP(S) `base`. Each authored screen maps
  the implementation to one path beginning with `/`, or to paths keyed by
  that screen's declared state IDs. A single path is used for every state.
  State maps need a path for the first/default state.
- The URL lens opens the external page in an iframe. Actions remain available
  in the external app; the workbench's Actions switch applies to served
  authored pages, not to external implementations. Apps must permit embedding
  and manage their own session in that context.
- A URL implementation may use the same optional `start` command and
  `check`/`ready` probes as Storybook. This is a VS Code startup feature, not
  a requirement to view a hosted implementation.
- An external page may opt into the cooperative preview bridge. Without it,
  [screenshot capture](capture.md) visits the URL in the helper's separate
  session; its authentication state may differ from the visible iframe.

## iOS Simulator implementations

```yaml
implementations:
  simulator:
    kind: ios-simulator
    device: booted
    catalog: true
```

- `device` selects all booted devices with `booted`, or one exact device name
  or UDID. With `catalog: true`, each matching booted device becomes an
  implementation-only screen. An authored screen may map a Simulator lens
  to a device name or UDID instead.
- The canvas uses the Simulator stream rather than an iframe. WDA carries
  taps and drags back to the device. `start` does not apply to Simulator
  implementations. When no matching booted device is found, the catalog
  reports a problem.
- Export reference screenshots are supported for authored design pages and
  imported Storybook stories. An implementation-only Simulator screen
  receives an export warning instead of a fabricated reference image.
  See the [export contract](export.md) for its capture plan and archive.

## Verification points

- [config.test.js](../../config.test.js) and
  [manifest.test.js](../../workbench/manifest.test.js) cover paths, state
  maps, and implementation validation.
- [server.test.js](../../server.test.js) covers serving, imports, and export
  warnings. [simulator.test.js](../../workbench/simulator.test.js) covers
  canvas stream behavior.
