import { randomUUID } from 'node:crypto';
import path from 'node:path';
import type { StateStore } from '../state/index.ts';
import type { GitWorkspaces } from '../git/index.ts';

export class Projects {
  readonly store: StateStore;
  readonly git: GitWorkspaces;
  constructor({ store, git }: { store: StateStore; git: GitWorkspaces }) {
    this.store = store;
    this.git = git;
  }
  find(id: string, state = this.store.snapshot()) {
    const project = state.projects.find((item) => item.id === id && item.open !== false);
    if (!project) throw new Error('Project does not exist.');
    return project;
  }
  async open(folder: string, name?: string) {
    const repository = await this.git.repository(folder);
    const branch = await this.git.branch(repository);
    return this.store.update((state) => {
      let project = state.projects.find((item) => item.repository === repository);
      if (!project) {
        const main = {
          id: randomUUID().slice(0, 8),
          name: 'Main',
          branch,
          checkout: repository,
          managed: false,
        };
        project = {
          id: randomUUID().slice(0, 8),
          name: name || path.basename(repository),
          repository,
          sessions: [main],
          activeSessionId: main.id,
        };
        state.projects.push(project);
      }
      project.open = true;
      state.activeProjectId = project.id;
      return project;
    });
  }
  select(projectId: string, sessionId?: string) {
    return this.store.update((state) => {
      const project = this.find(projectId, state);
      const selected = sessionId || project.activeSessionId;
      if (!project.sessions.some((item) => item.id === selected))
        throw new Error('Session does not belong to this project.');
      project.activeSessionId = selected;
      state.activeProjectId = projectId;
      return selected;
    });
  }
  close(id: string) {
    return this.store.update((state) => {
      this.find(id, state).open = false;
      if (state.activeProjectId === id)
        state.activeProjectId = state.projects.find((item) => item.open !== false)?.id || null;
      return id;
    });
  }
}
