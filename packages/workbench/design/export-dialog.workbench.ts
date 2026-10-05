import { defineDocs } from '@canonic2/workbench';
export default defineDocs({
  id: 'components/export-dialog', title: 'Components/Top bar/Export', icon: 'component', docs: './export-dialog.md',
  lenses: { docs: { label: 'Docs', adapter: 'html', examples: './export-dialog.examples.ts', styles: ['../src/theme/defaults.css', './design.css'] } },
});
