import { defineDocs } from '@canonic2/workbench';
export default defineDocs({
  id: 'components/preview-controls',
  title: 'Components/Top bar/Preview controls',
  icon: 'component',
  docs: './preview-controls.md',
  lenses: { docs: { label: 'Docs', adapter: 'html', examples: './preview-controls.examples.ts',
    styles: ['../ui/src/theme/defaults.css', './design.css'] } },
});
