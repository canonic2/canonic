import { errorMessage } from '../../src/platform/errors.ts';
import { app, BrowserWindow, ipcMain, Menu } from 'electron';
import path from 'node:path';
import { Workspaces } from './workspaces.ts';
import { createCommands } from './commands.ts';
import type { Studio } from '../../src/application/studio.ts';

export async function createDesktop({ manager, root }: { manager: Studio; root: string }) {
  const window = new BrowserWindow({
    width: 1280,
    height: 860,
    minWidth: 1000,
    minHeight: 640,
    title: 'Canonic Studio',
    backgroundColor: '#1f1f1f',
    webPreferences: {
      preload: path.join(root, 'app/preload.cjs'),
      sandbox: true,
      contextIsolation: true,
      nodeIntegration: false,
    },
  });
  const publish = () => {
    if (!window.isDestroyed()) window.webContents.send('studio:state', manager.snapshot());
  };
  const workspaces = new Workspaces({ window, studio: manager, publish });
  const handle = createCommands({ studio: manager, window, workspaces, publish });
  window.on('resize', () => workspaces.layout());
  window.webContents.setWindowOpenHandler(() => ({ action: 'deny' }));
  window.webContents.on('will-navigate', (event) => event.preventDefault());
  ipcMain.handle('studio', async (event, action: unknown, payload: unknown) => {
    if (event.sender !== window.webContents || event.senderFrame !== window.webContents.mainFrame)
      throw new Error('Invalid Studio caller.');
    try {
      if (typeof action !== 'string') throw new Error('Invalid Studio action.');
      return { ok: true, value: await handle(action, payload) };
    } catch (error) {
      return { ok: false, error: errorMessage(error) };
    }
  });
  Menu.setApplicationMenu(
    Menu.buildFromTemplate([
      { label: 'Studio', submenu: [{ role: 'about' }, { type: 'separator' }, { role: 'quit' }] },
      {
        label: 'Edit',
        submenu: [
          { role: 'undo' },
          { role: 'redo' },
          { type: 'separator' },
          { role: 'cut' },
          { role: 'copy' },
          { role: 'paste' },
          { role: 'selectAll' },
        ],
      },
      {
        label: 'View',
        submenu: [
          { role: 'togglefullscreen' },
          {
            label: 'Developer Tools',
            click: () =>
              (workspaces.active?.webContents || window.webContents).openDevTools({
                mode: 'detach',
              }),
          },
        ],
      },
    ]),
  );
  app.on('second-instance', () => {
    window.show();
    window.focus();
  });
  app.on('window-all-closed', () => app.quit());
  let quitting = false;
  app.on('before-quit', (event) => {
    if (quitting) return;
    event.preventDefault();
    quitting = true;
    workspaces.shutdown();
    manager
      .shutdown()
      .catch((error) => {
        console.error(error);
        process.exitCode = 1;
      })
      .finally(() => app.exit(Number(process.exitCode) || 0));
  });
  await window.loadFile(path.join(root, 'app/index.html'));
  publish();
  return { manager, window, views: workspaces.views, handle, root };
}
export type DesktopContext = Awaited<ReturnType<typeof createDesktop>>;
