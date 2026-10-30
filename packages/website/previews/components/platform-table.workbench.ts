import { defineDocs } from '@canonic2/workbench';
import { lens } from './fixtures';

export default defineDocs({
  id: 'website/platform-table',
  title: 'Components/Platform table',
  docs: './platform-table.md',
  lenses: lens('./platform-table/'),
});
