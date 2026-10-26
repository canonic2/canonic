import { WbSizesEditor } from './element.js';
import { register } from '../../../../components/register-elements.js';

export function registerSizesEditor(registry = customElements) {
  register(registry, [['wb-sizes-editor', WbSizesEditor]]);
}
