import { WbSizeSwitcher } from './components/size-switcher/element.js';
import { WbSizeDialog } from './components/size-dialog/element.js';
import { WbSizesEditor } from './components/sizes-editor/element.js';
import { register } from '../../components/register-elements.js';

export function registerSizeComponents(registry = customElements) {
  register(registry, [
    ['wb-size-switcher', WbSizeSwitcher],
    ['wb-size-dialog', WbSizeDialog],
    ['wb-sizes-editor', WbSizesEditor],
  ]);
}
