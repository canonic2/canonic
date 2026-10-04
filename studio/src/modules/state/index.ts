import { errorMessage, errorCode, asError } from '../../platform/errors.ts';
import { mkdir, readFile, rename, writeFile } from 'node:fs/promises';
import path from 'node:path';
import { Queue } from '../../platform/queue.ts';
import type { State, Cleanup } from './types.ts';

export class StateStore {
  readonly root: string;
  readonly queue: Queue;
  private value: State;
  constructor(root: string) {
    this.root = path.resolve(root);
    this.queue = new Queue();
    this.value = { version: 1, projects: [], activeProjectId: null };
  }
  async init() {
    await mkdir(this.root, { recursive: true });
    try {
      this.value = JSON.parse(await readFile(path.join(this.root, 'state.json'), 'utf8')) as State;
    } catch (error) {
      if (errorCode(error) !== 'ENOENT')
        throw new Error(`Cannot read Studio state: ${errorMessage(error)}`, { cause: error });
    }
    const ids = new Set();
    if (this.value.version !== 1 || !Array.isArray(this.value.projects))
      throw new Error('Unsupported Studio state.');
    for (const project of this.value.projects) {
      if (
        !project.id ||
        !project.name ||
        !path.isAbsolute(project.repository || '') ||
        !Array.isArray(project.sessions) ||
        ids.has(project.id)
      )
        throw new Error('Invalid project state.');
      ids.add(project.id);
      for (const session of project.sessions) {
        if (
          !session.id ||
          !session.name ||
          !session.branch ||
          !path.isAbsolute(session.checkout || '') ||
          ids.has(session.id)
        )
          throw new Error('Invalid session state.');
        ids.add(session.id);
      }
      if (!project.sessions.some((session) => session.id === project.activeSessionId))
        throw new Error('Invalid selected session.');
    }
    if (
      this.value.activeProjectId &&
      !this.value.projects.some(
        (project) => project.id === this.value.activeProjectId && project.open !== false,
      )
    )
      throw new Error('Invalid selected project.');
    return this;
  }
  snapshot() {
    return structuredClone(this.value);
  }
  update<T>(
    operation: (state: State, rollback: (cleanup: Cleanup) => void) => T | Promise<T>,
  ): Promise<T> {
    return this.queue.run(async () => {
      const next = this.snapshot();
      const rollback: Cleanup[] = [];
      try {
        const result = await operation(next, (cleanup) => rollback.push(cleanup));
        const file = path.join(this.root, 'state.json');
        await writeFile(`${file}.tmp`, JSON.stringify(next, null, 2), { mode: 0o600 });
        await rename(`${file}.tmp`, file);
        this.value = next;
        return structuredClone(result);
      } catch (error) {
        const results = await Promise.allSettled(
          rollback.reverse().map(async (cleanup) => cleanup()),
        );
        const failures = results.filter((result) => result.status === 'rejected');
        if (failures.length)
          throw new AggregateError(
            [error, ...failures.map((result) => asError(result.reason))],
            'State update and rollback failed.',
            { cause: error },
          );
        throw error;
      }
    });
  }
}
