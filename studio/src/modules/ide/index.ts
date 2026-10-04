import { errorMessage, errorCode, asError } from '../../platform/errors.ts';
import { mkdir, mkdtemp, readFile, rm, writeFile } from 'node:fs/promises';
import path from 'node:path';
import os from 'node:os';
import { freePort, launch, stopProcess, waitUntil } from '../../platform/processes.ts';
import { sessionEnvironment } from '../../platform/paths.ts';
import { Queue } from '../../platform/queue.ts';
import { loadProject } from '../configuration/index.ts';
import type { Sessions } from '../sessions/index.ts';
import type { ProjectConfig } from '../configuration/types.ts';
import type { ProcessRecord } from '../../platform/processes.ts';

export interface IDERecord {
  process: ProcessRecord;
  url: string;
  port: number;
  socketDirectory: string;
}

export class IDEs {
  readonly root: string;
  readonly runtime: string;
  readonly sessions: Sessions;
  readonly records = new Map<string, IDERecord>();
  readonly errors = new Map<string, string>();
  readonly queues = new Map<string, Queue>();
  constructor({ root, runtime, sessions }: { root: string; runtime: string; sessions: Sessions }) {
    this.root = root;
    this.runtime = runtime;
    this.sessions = sessions;
  }
  queue(id: string) {
    if (!this.queues.has(id)) this.queues.set(id, new Queue());
    return this.queues.get(id)!;
  }
  async start(id: string) {
    return this.queue(id).run(async () => {
      const current = this.records.get(id);
      if (current?.process && !current.process.exited) return current;
      const { session } = this.sessions.find(id);
      const data = path.join(this.root, 'sessions', id, 'ide');
      await mkdir(path.join(data, 'User'), { recursive: true });
      let config: ProjectConfig = { version: 1 };
      let configError: string | undefined;
      try {
        config = await loadProject(session.checkout);
      } catch (error) {
        configError = errorMessage(error);
      }
      try {
        await readFile(path.join(data, 'User/settings.json'));
      } catch (error) {
        if (errorCode(error) !== 'ENOENT') throw error;
        await writeFile(
          path.join(data, 'User/settings.json'),
          JSON.stringify(
            {
              'workbench.colorTheme': 'Default Dark Modern',
              'workbench.startupEditor': 'none',
              'window.menuBarVisibility': 'compact',
              'telemetry.telemetryLevel': 'off',
              'security.workspace.trust.enabled': true,
              'extensions.autoUpdate': 'off',
              'chat.disableAIFeatures': true,
              'workbench.secondarySideBar.defaultVisibility': 'hidden',
              ...config.ide?.settings,
            },
            null,
            2,
          ),
        );
      }
      const port = await freePort();
      const url = `http://127.0.0.1:${port}/?folder=${encodeURIComponent(session.checkout)}`;
      const env = sessionEnvironment(this.root, session);
      // Unix socket names have a much smaller limit than ordinary file paths.
      const socketDirectory = await mkdtemp(path.join(os.tmpdir(), 'studio-ide-'));
      const record = launch(
        this.runtime,
        [
          '--bind-addr',
          `127.0.0.1:${port}`,
          '--auth',
          'none',
          '--disable-telemetry',
          '--disable-update-check',
          '--user-data-dir',
          data,
          '--session-socket',
          path.join(socketDirectory, 'ipc.sock'),
          '--extensions-dir',
          path.join(this.root, 'sessions', id, 'extensions'),
          session.checkout,
        ],
        { cwd: session.checkout, env: { ...env, PORT: String(port) } },
      );
      const ide = { process: record, url, port, socketDirectory };
      this.records.set(id, ide);
      try {
        await waitUntil(
          record,
          async () => {
            if (!record.output.includes(`HTTP server listening on http://127.0.0.1:${port}`))
              return false;
            try {
              return (
                await fetch(`http://127.0.0.1:${port}/healthz`, {
                  signal: AbortSignal.timeout(800),
                })
              ).ok;
            } catch {
              return false;
            }
          },
          60000,
        );
        if (configError) this.errors.set(id, configError);
        else this.errors.delete(id);
        return ide;
      } catch (error) {
        await stopProcess(record);
        await rm(socketDirectory, { recursive: true, force: true });
        this.records.delete(id);
        this.errors.set(id, errorMessage(error));
        throw new Error(
          `Could not start the IDE. Run pnpm run setup first. ${errorMessage(error)}`,
          { cause: error },
        );
      }
    });
  }

  async stop(id: string) {
    return this.queue(id).run(async () => {
      const ide = this.records.get(id);
      await stopProcess(ide?.process);
      if (ide?.socketDirectory) await rm(ide.socketDirectory, { recursive: true, force: true });
      this.records.delete(id);
    });
  }
  async shutdown() {
    const results = await Promise.allSettled([...this.queues.keys()].map((id) => this.stop(id)));
    const failures = results.filter((result) => result.status === 'rejected');
    if (failures.length)
      throw new AggregateError(
        failures.map((result) => asError(result.reason)),
        'IDE shutdown failed',
      );
  }
}
