import { randomUUID } from 'node:crypto';
import path from 'node:path';
import type { StateStore } from '../state/index.ts';
import type { Projects } from '../projects/index.ts';
import type { GitWorkspaces } from '../git/index.ts';

export class Sessions {
  readonly store: StateStore;
  readonly projects: Projects;
  readonly git: GitWorkspaces;
  constructor({
    store,
    projects,
    git,
  }: {
    store: StateStore;
    projects: Projects;
    git: GitWorkspaces;
  }) {
    this.store = store;
    this.projects = projects;
    this.git = git;
  }
  find(id: string) {
    for (const project of this.store.snapshot().projects.filter((item) => item.open !== false)) {
      const session = project.sessions.find((item) => item.id === id);
      if (session) return { project, session };
    }
    throw new Error('Session does not exist.');
  }
  create(projectId: string, name: string, base = 'HEAD') {
    return this.store.update(async (state, rollback) => {
      const project = this.projects.find(projectId, state);
      name = typeof name === 'string' ? name.trim() : '';
      if (!name || name.length > 80) throw new Error('Give the session a name of 1–80 characters.');
      if (typeof base !== 'string' || !base.trim())
        throw new Error('Choose a starting branch or commit.');
      const id = randomUUID().slice(0, 8);
      const checkout = path.join(this.store.root, 'worktrees', project.id, id);
      const slug =
        name
          .toLowerCase()
          .replace(/[^a-z0-9]+/g, '-')
          .replace(/^-|-$/g, '') || 'task';
      const branch = `codex/studio/${slug}-${id}`;
      await this.git.create(project.repository, checkout, branch, base);
      rollback(() => this.git.rollback(project.repository, checkout, branch));
      const session = { id, name, branch, checkout, managed: true };
      project.sessions.push(session);
      project.activeSessionId = id;
      state.activeProjectId = project.id;
      return session;
    });
  }
}
