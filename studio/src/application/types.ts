import type { Project, Session, State } from '../modules/state/types.ts';

export interface SessionSnapshot extends Session {
  ideStatus: 'ready' | 'idle';
  serviceStatus: 'starting' | 'stopping' | 'running' | 'stopped';
  serviceUrl: string | null;
  ports: Record<string, number>;
  composeStatus: 'running' | 'stopped';
  composeName: string;
  error: string | null;
}

export interface ProjectSnapshot extends Omit<Project, 'sessions'> {
  sessions: SessionSnapshot[];
}

export interface StudioSnapshot extends Omit<State, 'projects'> {
  projects: ProjectSnapshot[];
}
