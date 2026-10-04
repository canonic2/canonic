import type { StudioSnapshot } from '../src/application/types.ts';

export type WorkspaceMode = 'code' | 'preview' | 'logs' | 'blank';
export interface Requests {
  state: undefined;
  logs: { sessionId: string };
  select: { projectId: string; sessionId?: string };
  workspace: { sessionId: string; mode: WorkspaceMode };
  newSession: { projectId: string; name: string; base?: string };
  openProject: undefined;
  closeProject: { projectId: string };
  startService: { sessionId: string };
  stopService: { sessionId: string };
}
export type Action = keyof Requests;
export type Response<A extends Action> = A extends 'logs' ? string : StudioSnapshot;
type IPCResult<T> = { ok: true; value: T } | { ok: false; error: string };
export interface StudioBridge {
  invoke<A extends Action>(action: A, payload?: Requests[A]): Promise<IPCResult<Response<A>>>;
  onState(callback: (state: StudioSnapshot) => void): () => void;
}
declare global {
  interface Window {
    studio: StudioBridge;
  }
}
