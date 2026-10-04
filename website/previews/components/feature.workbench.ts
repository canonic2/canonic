import { defineDocs } from '@canonic2/workbench';
import { lens } from './fixtures';

export default defineDocs({
  id: 'website/feature',
  title: 'Components/Feature',
  docs: './feature.md',
  lenses: lens('./feature/'),
});
