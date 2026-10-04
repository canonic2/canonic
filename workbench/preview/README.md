# @canonic2/workbench

Types for [Canonic Workbench](https://canonic.sh/workbench/) previews:
`definePreview`, `defineConfig`, and `defineAdapter`.

```sh
npm install --save-dev @canonic2/workbench
```

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

The package lets TypeScript and your editor check preview definitions. It
checks that each state's `inputs` match the preview's. When Workbench compiles
a preview, it supplies `@canonic2/workbench` itself, so the version installed
here affects types only.

See [Workbench previews](https://canonic.sh/workbench/docs/workbench-previews/).

Canonic is proprietary; all rights reserved. See [LICENSE](LICENSE).
