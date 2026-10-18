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
      styles: ['../ui/legacy/page-list.css', '../ui/src/theme/defaults.css', '../ui/legacy/sidebar.css', './design.css'],
    },
  },
});
