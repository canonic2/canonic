import { defineDocs } from '@canonic2/workbench';
import { lens } from './fixtures';

export default defineDocs({
  id: 'website/release-entry',
  title: 'Components/Release entry',
  docs: './release-entry.md',
  lenses: lens('./release-entry/'),
});
