import { WbSizeSwitcher } from './element.js';
import { register } from '../../../../components/register-elements.js';

export function registerSizeSwitcher(registry = customElements) {
  register(registry, [['wb-size-switcher', WbSizeSwitcher]]);
}
