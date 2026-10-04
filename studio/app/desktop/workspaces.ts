import { WebContentsView, shell } from 'electron';
import type { BrowserWindow } from 'electron';
import type { Studio } from '../../src/application/studio.ts';
import type { WorkspaceMode } from '../contracts.ts';
type WorkspaceView = WebContentsView & { studioURL?: string };

export class Workspaces {
  readonly window: BrowserWindow;
  readonly studio: Studio;
  readonly publish: () => void;
  readonly views = new Map<string, WorkspaceView>();
  generation = 0;
  active: WorkspaceView | null = null;
  constructor({
    window,
    studio,
    publish,
  }: {
    window: BrowserWindow;
    studio: Studio;
    publish: () => void;
  }) {
    this.window = window;
    this.studio = studio;
    this.publish = publish;
  }
  detach() {
    if (this.active && !this.window.isDestroyed())
      this.window.contentView.removeChildView(this.active);
    this.active = null;
  }
  layout() {
    if (!this.active || this.window.isDestroyed()) return;
    const [width, height] = this.window.getContentSize();
    this.active.setBounds({
      x: 224,
      y: 106,
      width: Math.max(1, width - 224),
      height: Math.max(1, height - 132),
    });
  }
  async show(id: string, mode: WorkspaceMode = 'code') {
    if (!['code', 'preview', 'logs', 'blank'].includes(mode))
      throw new Error('Unknown workspace view.');
    this.studio.sessions.find(id);
    const request = ++this.generation;
    this.detach();
    if (mode === 'logs' || mode === 'blank') return;
    const runtime =
      mode === 'code' ? await this.studio.ensureIDE(id) : this.studio.runtimes.records.get(id);
    if (!runtime?.process || runtime.process.exited)
      throw new Error('Start this session before opening its preview.');
    if (request !== this.generation || this.window.isDestroyed()) return;
    const key = `${id}:${mode}`;
    let view = this.views.get(key);
    if (!view) {
      view = new WebContentsView({
        webPreferences: {
          partition: `persist:studio-${id}`,
          contextIsolation: true,
          nodeIntegration: false,
          sandbox: true,
          backgroundThrottling: false,
        },
      });
      view.setBackgroundColor('#1f1f1f');
      view.webContents.setWindowOpenHandler(({ url }) => {
        if (/^https?:\/\//.test(url))
          void shell.openExternal(url).catch((error) => console.error(error));
        return { action: 'deny' };
      });
      view.webContents.on('will-navigate', (event, url) => {
        if (new URL(url).origin !== new URL(view!.studioURL!).origin) {
          event.preventDefault();
          if (/^https?:\/\//.test(url))
            void shell.openExternal(url).catch((error) => console.error(error));
        }
      });
      this.views.set(key, view);
    }
    this.window.contentView.addChildView(view);
    this.active = view;
    this.layout();
    const target = mode === 'code' ? runtime.url : `${runtime.url}/`;
    if (view.studioURL !== target) {
      view.studioURL = target;
      await view.webContents.loadURL(target);
    }
    if (request === this.generation) view.webContents.focus();
    this.publish();
  }
  close(id?: string, mode?: WorkspaceMode) {
    for (const [key, view] of this.views) {
      if ((id && !key.startsWith(`${id}:`)) || (mode && !key.endsWith(`:${mode}`))) continue;
      if (this.active === view) this.detach();
      if (!view.webContents.isDestroyed()) view.webContents.close();
      this.views.delete(key);
    }
  }
  shutdown() {
    this.generation++;
    this.close();
  }
}
