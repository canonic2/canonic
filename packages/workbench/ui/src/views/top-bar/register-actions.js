import { WbActionsSwitch } from './components/actions-switch/element.js';
import { register } from '../../bootstrap/register-elements.js';

export function registerActionsComponents(registry = customElements) {
  register(registry, [['wb-actions-switch', WbActionsSwitch]]);
}
