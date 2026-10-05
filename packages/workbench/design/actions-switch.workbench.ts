import { defineDocs } from '@canonic2/workbench';

export default defineDocs({
  id: 'components/actions-switch',
  title: 'Components/Top bar/Actions switch',
  icon: 'component',
  docs: './actions-switch.md',
  lenses: {
    docs: { label: 'Docs', adapter: 'html', examples: './actions-switch.examples.ts', styles: ['../src/theme/defaults.css', './design.css'] },
  },
});
