import { defineDocs } from '@canonic2/workbench';
export default defineDocs({
  id: 'components/canvas',
  title: 'Components/Canvas/Artboards',
  docs: './canvas.md',
  lenses: { docs: { label: 'Docs', adapter: 'html', examples: './canvas.examples.ts', styles: ['../ui/src/theme/defaults.css', './design.css'] } },
});
