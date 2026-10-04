import { defineDocs } from '@canonic2/workbench';
import { lens } from './fixtures';

export default defineDocs({
  id: 'website/download-cards',
  title: 'Components/Download cards',
  docs: './download-cards.md',
  lenses: lens('./download-cards/'),
});
