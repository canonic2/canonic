import { definePreview } from '@canonic2/workbench';
import { downloads, styles, withScripts } from './fixtures';

// The panel shows once the element recognizes this computer's system.
export default definePreview({
  id: 'website/recommended-download',
  title: 'Components/Recommended download',
  adapter: 'astro',
  source: { entry: '../../src/components/RecommendedDownload.astro' },
  styles,
  setup: withScripts,
  inputs: { downloads },
  viewports: ['desktop', 'mobile', 'responsive'],
});
