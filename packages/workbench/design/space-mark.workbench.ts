import { defineDocs } from '@canonic2/workbench';

// Drawn in the sidebar's colors, so the lens carries the sidebar's styles.
export default defineDocs({
  id: 'components/space-mark',
  title: 'Components/Sidebar/Space mark',
  icon: 'component',
  docs: './space-mark.md',
  lenses: {
    docs: {
      label: 'Docs', adapter: 'html', examples: './space-mark.examples.ts',
      styles: ['../ui/legacy/page-list.css', '../ui/src/theme/defaults.css', '../ui/legacy/sidebar.css', './design.css'],
    },
  },
});
