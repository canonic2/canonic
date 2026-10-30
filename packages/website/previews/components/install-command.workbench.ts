import { defineDocs } from '@canonic2/workbench';
import { lens } from './fixtures';

export default defineDocs({
  id: 'website/install-command',
  title: 'Components/Install command',
  docs: './install-command.md',
  lenses: lens('./install-command/'),
});
