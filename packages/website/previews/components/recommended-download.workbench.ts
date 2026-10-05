import { defineDocs } from '@canonic2/workbench';
import { lens } from './fixtures';

export default defineDocs({
  id: 'website/recommended-download',
  title: 'Components/Recommended download',
  docs: './recommended-download.md',
  lenses: lens('./recommended-download/'),
});
