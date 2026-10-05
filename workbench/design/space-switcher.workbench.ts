import { defineDocs } from '@canonic2/workbench';

// Drawn in the sidebar's colors, so the lens carries the sidebar's styles.
export default defineDocs({
  id: 'components/space-switcher',
  title: 'Components/Sidebar/Space switcher',
  icon: 'component',
  docs: './space-switcher.md',
  lenses: {
    docs: {
      label: 'Docs', adapter: 'html', examples: './space-switcher.examples.ts',
      styles: ['../workbench/page-list.css', '../src/theme/defaults.css', '../workbench/sidebar.css', './design.css'],
    },
  },
});
