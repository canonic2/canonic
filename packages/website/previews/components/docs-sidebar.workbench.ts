import { defineDocs } from '@canonic2/workbench';
import { lens } from './fixtures';

export default defineDocs({
  id: 'website/docs-sidebar',
  title: 'Components/Docs sidebar',
  docs: './docs-sidebar.md',
  lenses: lens('./docs-sidebar/', ['../../src/styles/docs.css']),
});
