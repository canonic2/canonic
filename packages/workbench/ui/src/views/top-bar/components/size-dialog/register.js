import { WbSizeDialog } from './element.js';
import { register } from '../../../../components/register-elements.js';

export function registerSizeDialog(registry = customElements) {
  register(registry, [['wb-size-dialog', WbSizeDialog]]);
}
