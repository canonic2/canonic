import { registerSizeSwitcher } from './components/size-switcher/register.js';
import { registerSizeDialog } from './components/size-dialog/register.js';
import { registerSizesEditor } from './components/sizes-editor/register.js';

export function registerSizeComponents(registry = customElements) {
  registerSizeSwitcher(registry);
  registerSizeDialog(registry);
  registerSizesEditor(registry);
}
