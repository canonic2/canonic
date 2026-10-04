import { asError } from '../platform/errors.ts';
import path from 'node:path';
import { StateStore } from '../modules/state/index.ts';
import { GitWorkspaces } from '../modules/git/index.ts';
import { Projects } from '../modules/projects/index.ts';
import { Sessions } from '../modules/sessions/index.ts';
import { IDEs } from '../modules/ide/index.ts';
import { Runtimes } from '../modules/runtime/index.ts';
import { defaultRuntime, packageRoot } from '../platform/paths.ts';
import type { StudioSnapshot } from './types.ts';

export class Studio {
  readonly store: StateStore;
  readonly root: string;
  readonly projects: Projects;
  readonly sessions: Sessions;
  readonly ide: IDEs;
  readonly runtimes: Runtimes;
  readonly closing = new Set<string>();
  quitting = false;
  constructor({ root = path.join(packageRoot, '.studio/app'), runtime = defaultRuntime } = {}) {
    this.store = new StateStore(root);
    this.root = this.store.root;
    const git = new GitWorkspaces();
    this.projects = new Projects({ store: this.store, git });
    this.sessions = new Sessions({ store: this.store, projects: this.projects, git });
    this.ide = new IDEs({ root: this.root, runtime, sessions: this.sessions });
    this.runtimes = new Runtimes({ root: this.root, sessions: this.sessions });
  }
  async init() {
    await this.store.init();
    return this;
  }
  assertAvailable(id?: string) {
    if (this.quitting || (id !== undefined && this.closing.has(id)))
      throw new Error('Studio is closing this workspace.');
  }
  addProject(folder: string, name?: string) {
    this.assertAvailable();
    return this.projects.open(folder, name);
  }
  createSession(projectId: string, name: string, base?: string) {
    this.assertAvailable(projectId);
    return this.sessions.create(projectId, name, base);
  }
  select(projectId: string, sessionId?: string) {
    this.assertAvailable(projectId);
    return this.projects.select(projectId, sessionId);
  }
  ensureIDE(id: string) {
    this.assertAvailable(this.sessions.find(id).project.id);
    return this.ide.start(id);
  }
  startService(id: string) {
    this.assertAvailable(this.sessions.find(id).project.id);
    return this.runtimes.start(id);
  }
  stopService(id: string) {
    this.sessions.find(id);
    return this.runtimes.stop(id);
  }
  logs(id: string) {
    return this.runtimes.logs(id);
  }
  async closeProject(id: string) {
    this.assertAvailable(id);
    this.closing.add(id);
    try {
      await this.store.queue.pending;
      const project = this.projects.find(id);
      const results = await Promise.allSettled(
        project.sessions.flatMap((session) => [
          this.runtimes.stop(session.id),
          this.ide.stop(session.id),
        ]),
      );
      const failures = results.filter((result) => result.status === 'rejected');
      if (failures.length)
        throw new AggregateError(
          failures.map((result) => asError(result.reason)),
          'Could not stop all project sessions.',
        );
      await this.projects.close(id);
    } finally {
      this.closing.delete(id);
    }
  }
  snapshot(): StudioSnapshot {
    const state = this.store.snapshot();
    return {
      ...state,
      projects: state.projects
        .filter((project) => project.open !== false)
        .map((project) => ({
          ...project,
          sessions: project.sessions.map((session) => {
            const ide = this.ide.records.get(session.id);
            const task = this.runtimes.records.get(session.id);
            const running = task?.process && !task.process.exited;
            return {
              ...session,
              ideStatus: ide && !ide.process.exited ? 'ready' : 'idle',
              serviceStatus:
                this.runtimes.status.get(session.id) || (running ? 'running' : 'stopped'),
              serviceUrl: running ? task.url : null,
              ports: task?.ports || {},
              composeStatus: task?.composeStarted ? 'running' : 'stopped',
              composeName: `studio_${project.id}_${session.id}`,
              error:
                this.runtimes.errors.get(session.id) || this.ide.errors.get(session.id) || null,
            };
          }),
        })),
    };
  }
  async shutdown() {
    this.quitting = true;
    await this.store.queue.pending;
    const results = await Promise.allSettled([this.runtimes.shutdown(), this.ide.shutdown()]);
    const failures = results.filter((result) => result.status === 'rejected');
    if (failures.length)
      throw new AggregateError(
        failures.map((result) => asError(result.reason)),
        'Studio shutdown failed',
      );
  }
}
