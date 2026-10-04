import { Queue } from '../../platform/queue.ts';
import { sessionEnvironment } from '../../platform/paths.ts';
import { loadProject } from '../configuration/index.ts';
import { startRuntime, stopRuntime } from './supervisor.ts';
import type { Sessions } from '../sessions/index.ts';
import type { RuntimeTask, RuntimeFailure } from './types.ts';
import { asError } from '../../platform/errors.ts';

export class Runtimes {
  readonly root: string;
  readonly sessions: Sessions;
  readonly records = new Map<string, RuntimeTask>();
  readonly errors = new Map<string, string>();
  readonly queues = new Map<string, Queue>();
  readonly status = new Map<string, 'starting' | 'stopping'>();
  constructor({ root, sessions }: { root: string; sessions: Sessions }) {
    this.root = root;
    this.sessions = sessions;
  }
  queue(id: string) {
    if (!this.queues.has(id)) this.queues.set(id, new Queue());
    return this.queues.get(id)!;
  }
  start(id: string) {
    this.sessions.find(id);
    return this.queue(id).run(async () => {
      const current = this.records.get(id);
      if (current?.process && !current.process.exited) return current;
      this.status.set(id, 'starting');
      try {
        const { project, session } = this.sessions.find(id);
        const { runtime: recipe } = await loadProject(session.checkout);
        if (!recipe)
          throw new Error('Add a runtime to studio.config.json before starting this session.');
        if (current) {
          await stopRuntime(current);
          this.records.delete(id);
        }
        const task = await startRuntime({
          recipe,
          project,
          session,
          env: sessionEnvironment(this.root, session),
        });
        this.records.set(id, task);
        this.errors.delete(id);
        return task;
      } catch (error) {
        const failure: RuntimeFailure = asError(error);
        if (failure.cleanupTask) this.records.set(id, failure.cleanupTask);
        this.errors.set(id, failure.message);
        throw failure;
      } finally {
        this.status.delete(id);
      }
    });
  }
  stop(id: string) {
    return this.queue(id).run(async () => {
      this.status.set(id, 'stopping');
      try {
        await stopRuntime(this.records.get(id));
        this.records.delete(id);
      } finally {
        this.status.delete(id);
      }
    });
  }
  logs(id: string) {
    this.sessions.find(id);
    const task = this.records.get(id);
    return task
      ? (task.composeOutput || '') + (task.process?.output || '')
      : 'No runtime output yet.';
  }
  async shutdown() {
    const results = await Promise.allSettled([...this.queues.keys()].map((id) => this.stop(id)));
    const failures = results.filter((result) => result.status === 'rejected');
    if (failures.length)
      throw new AggregateError(
        failures.map((result) => asError(result.reason)),
        'Runtime shutdown failed',
      );
  }
}
