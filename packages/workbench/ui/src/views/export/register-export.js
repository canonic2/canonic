import { WbExportDialog } from './components/export-dialog/element.js';
import { register } from '../../components/register-elements.js';

export function registerExportComponents(registry = customElements) {
  register(registry, [['wb-export-dialog', WbExportDialog]]);
}
