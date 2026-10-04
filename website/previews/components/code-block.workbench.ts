import { defineDocs } from '@canonic2/workbench';
import { lens } from './fixtures';

export default defineDocs({
  id: 'website/code-block',
  title: 'Components/Code block',
  docs: './code-block.md',
  lenses: lens('./code-block/'),
});
