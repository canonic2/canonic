import { defineDocs } from '@canonic2/workbench';
import { lens } from './fixtures';

export default defineDocs({
  id: 'website/section-head',
  title: 'Components/Section head',
  docs: './section-head.md',
  lenses: lens('./section-head/'),
});
