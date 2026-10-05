import { defineDocs } from '@canonic2/workbench';

export default defineDocs({
  id: 'components/size-dialog',
  title: 'Components/Top bar/Custom size',
  icon: 'component',
  docs: './size-dialog.md',
  lenses: {
    docs: { label: 'Docs', adapter: 'html', examples: './size-dialog.examples.ts', styles: ['../src/theme/defaults.css', './design.css'] },
  },
});
