export interface Session {
  id: string;
  name: string;
  branch: string;
  checkout: string;
  managed: boolean;
}

export interface Project {
  id: string;
  name: string;
  repository: string;
  sessions: Session[];
  activeSessionId: string;
  open?: boolean;
}

export interface State {
  version: 1;
  projects: Project[];
  activeProjectId: string | null;
}

export type Cleanup = () => void | Promise<void>;
