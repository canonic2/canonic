import { definePreview } from '@canonic2/workbench';
import { styles, withScripts } from './fixtures';

export default definePreview({
  id: 'website/code-block',
  title: 'Components/Code block',
  adapter: 'astro',
  source: { entry: '../../src/components/CodeBlock.astro' },
  styles,
  setup: withScripts,
  inputs: {
    file: 'workbench.yaml',
    code: 'name: Acme\n\nsections:\n  - name: Pages\n    items:\n      - label: Sign in\n        src: pages/sign-in.html',
    copies: 'the workbench.yaml example',
  },
  controls: { file: { type: 'text' }, code: { type: 'text' }, copies: { type: 'text' } },
  states: {
    default: { label: 'workbench.yaml' },
    'one-line': { label: 'One line', inputs: { file: 'address', code: '#pages/sign-in.html:error@393~staging', copies: 'the address example' } },
  },
  viewports: ['fit', 'responsive'],
});
