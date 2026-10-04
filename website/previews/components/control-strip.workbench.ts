import { definePreview } from '@canonic2/workbench';
import { styles, withScripts } from './fixtures';

export default definePreview({
  id: 'website/control-strip',
  title: 'Components/Control strip',
  adapter: 'astro',
  source: { entry: './ControlStrip.example.astro' },
  styles,
  setup: withScripts,
  viewports: ['desktop', 'mobile', 'responsive'],
});
