import { defineDocs } from '@canonic2/workbench';

export default defineDocs({
  id: 'components/sizes-editor',
  title: 'Components/Top bar/Edit sizes',
  icon: 'component',
  docs: './sizes-editor.md',
  lenses: {
    docs: { label: 'Docs', adapter: 'html', examples: './sizes-editor.examples.ts', styles: ['../src/theme/defaults.css', './design.css'] },
  },
});
