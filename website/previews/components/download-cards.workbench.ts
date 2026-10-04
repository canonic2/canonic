import { definePreview } from '@canonic2/workbench';
import { downloads, styles, withScripts } from './fixtures';

export default definePreview({
  id: 'website/download-cards',
  title: 'Components/Download cards',
  adapter: 'astro',
  source: { entry: '../../src/components/DownloadCards.astro' },
  styles,
  setup: withScripts,
  inputs: { downloads },
  states: {
    default: { label: 'Release' },
    'no-release': { label: 'Before the first release', inputs: { downloads: null } },
  },
  viewports: ['desktop', 'mobile', 'responsive'],
});
