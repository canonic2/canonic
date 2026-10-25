import { WbExportDialog } from '../ui/src/views/export/components/export-dialog/element.js';
import { registerExportComponents } from '../ui/src/views/export/register-export.js';

registerExportComponents();
export const currentPage = (canvas: HTMLElement) => {
  const dialog = new WbExportDialog();
  dialog.pages = [{ id: 'home.html', label: 'Home' }, { id: 'card.html', label: 'Card' }];
  dialog.collections = ['Pages'];
  dialog.current = { page: 'home.html', width: 1440, height: 900 };
  const button = document.createElement('button'); button.textContent = 'Export…';
  button.addEventListener('click', () => dialog.show());
  canvas.append(button, dialog);
};
