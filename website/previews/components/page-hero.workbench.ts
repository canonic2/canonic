import { definePreview } from '@canonic2/workbench';
import { styles } from './fixtures';

export default definePreview({
  id: 'website/page-hero',
  title: 'Components/Page hero',
  adapter: 'astro',
  source: { entry: './PageHero.example.astro' },
  styles,
  inputs: { kicker: 'Install · v1.2.0', title: 'Install Canonic Workbench', width: 'narrow' },
  controls: { kicker: { type: 'text' }, title: { type: 'text' }, width: { type: 'select', options: ['narrow', 'narrower'] } },
  states: {
    default: { label: 'Narrow' },
    narrower: { label: 'Narrower', inputs: { kicker: 'Workbench', title: 'Changelog', width: 'narrower' } },
  },
  viewports: ['desktop', 'mobile', 'responsive'],
});
