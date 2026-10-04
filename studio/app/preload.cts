import { contextBridge, ipcRenderer } from 'electron';
import type { IpcRendererEvent } from 'electron';
import type { StudioBridge } from './contracts.ts';
import type { StudioSnapshot } from '../src/application/types.ts';

const bridge: StudioBridge = {
  invoke: (action, payload) => ipcRenderer.invoke('studio', action, payload),
  onState: (callback) => {
    const listener = (_event: IpcRendererEvent, state: StudioSnapshot) => callback(state);
    ipcRenderer.on('studio:state', listener);
    return () => ipcRenderer.removeListener('studio:state', listener);
  },
};
contextBridge.exposeInMainWorld('studio', bridge);
