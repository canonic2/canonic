import { definePreview } from '@canonic2/workbench';
import { styles, withScripts } from './fixtures';

// The command names this computer's .vsix once the element runs.
export default definePreview({
  id: 'website/install-command',
  title: 'Components/Install command',
  adapter: 'astro',
  source: { entry: '../../src/components/InstallCommand.astro' },
  styles,
  setup: withScripts,
  viewports: ['fit', 'responsive'],
});
