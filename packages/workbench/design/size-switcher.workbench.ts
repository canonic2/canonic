import { defineDocs } from '@canonic2/workbench';

export default defineDocs({
  id: 'components/size-switcher',
  title: 'Components/Top bar/Size switcher',
  icon: 'component',
  docs: './size-switcher.md',
  lenses: {
    docs: { label: 'Docs', adapter: 'html', examples: './size-switcher.examples.ts', styles: ['../src/theme/defaults.css', './design.css'] },
  },
});
