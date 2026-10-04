import { defineDocs } from '@canonic2/workbench';
import { lens } from './fixtures';

export default defineDocs({
  id: 'website/page-hero',
  title: 'Components/Page hero',
  docs: './page-hero.md',
  lenses: lens('./page-hero/'),
});
