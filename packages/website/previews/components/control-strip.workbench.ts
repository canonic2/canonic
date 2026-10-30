import { defineDocs } from '@canonic2/workbench';
import { lens } from './fixtures';

export default defineDocs({
  id: 'website/control-strip',
  title: 'Components/Control strip',
  docs: './control-strip.md',
  lenses: lens('./control-strip/'),
});
