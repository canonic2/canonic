import { definePreview } from '@canonic2/workbench';
import { downloads, styles, withScripts } from './fixtures';

export default definePreview({
  id: 'website/platform-table',
  title: 'Components/Platform table',
  adapter: 'astro',
  source: { entry: '../../src/components/PlatformTable.astro' },
  styles,
  setup: withScripts,
  inputs: { downloads },
  viewports: ['desktop', 'mobile', 'responsive'],
});
