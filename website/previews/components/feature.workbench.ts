import { definePreview } from '@canonic2/workbench';
import { styles, withScripts } from './fixtures';

export default definePreview({
  id: 'website/feature',
  title: 'Components/Feature',
  adapter: 'astro',
  source: { entry: './Feature.example.astro' },
  styles,
  setup: withScripts,
  inputs: { kicker: 'States', title: 'Screens with several states' },
  controls: { kicker: { type: 'text' }, title: { type: 'text' } },
  viewports: ['desktop', 'mobile', 'responsive'],
});
