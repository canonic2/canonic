import { errorCode } from './errors.ts';
import { spawn } from 'node:child_process';
import net from 'node:net';
import type { ChildProcess, SpawnOptions } from 'node:child_process';

export interface ProcessRecord {
  child: ChildProcess;
  output: string;
  exited: boolean;
  error: Error | null;
  code?: number | null;
  signal?: NodeJS.Signals | null;
  done: Promise<void>;
}

export async function freePort() {
  const server = net.createServer();
  await new Promise<void>((resolve, reject) => {
    server.once('error', reject);
    server.listen(0, '127.0.0.1', resolve);
  });
  const port = (server.address() as net.AddressInfo).port;
  await new Promise<void>((resolve) => server.close(() => resolve()));
  return port;
}

export function launch(command: string, args: string[], options: SpawnOptions = {}): ProcessRecord {
  const child = spawn(command, args, {
    ...options,
    detached: true,
    stdio: ['ignore', 'pipe', 'pipe'],
  });
  const record: ProcessRecord = {
    child,
    output: '',
    exited: false,
    error: null,
    done: Promise.resolve(),
  };
  record.done = new Promise<void>((resolve) => {
    child.once('error', (error) => {
      record.error = error;
      record.exited = true;
      resolve();
    });
    child.once('exit', (code, signal) => {
      record.exited = true;
      record.code = code;
      record.signal = signal;
      resolve();
    });
  });
  for (const stream of [child.stdout, child.stderr]) {
    stream.on('data', (data) => {
      record.output = (record.output + data).slice(-32000);
    });
  }
  return record;
}

export async function waitUntil(
  record: ProcessRecord,
  check: () => boolean | Promise<boolean>,
  timeout = 30000,
) {
  const deadline = Date.now() + timeout;
  while (Date.now() < deadline) {
    if (record.exited)
      throw new Error(
        record.error?.message || `Process exited (${record.code}): ${record.output.slice(-1800)}`,
      );
    if (await check()) return;
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  throw new Error(`Service did not become ready: ${record.output.slice(-1800)}`);
}

export async function waitForExit(record: ProcessRecord, timeout: number) {
  let timer: NodeJS.Timeout | undefined;
  try {
    await Promise.race([
      record.done,
      new Promise((_resolve, reject) => {
        timer = setTimeout(
          () =>
            reject(
              new Error(
                `Provision command timed out after ${timeout}ms: ${record.output.slice(-1800)}`,
              ),
            ),
          timeout,
        );
      }),
    ]);
    if (record.error) throw record.error;
    if (record.code !== 0)
      throw new Error(
        `Provision command exited (${record.code ?? record.signal}): ${record.output.slice(-1800)}`,
      );
  } finally {
    clearTimeout(timer);
  }
}

export async function stopProcess(record?: ProcessRecord | null) {
  if (!record?.child.pid) return;
  const pid = record.child.pid;
  const signal = (name: NodeJS.Signals) => {
    try {
      process.kill(-pid, name);
    } catch (error) {
      if (errorCode(error) !== 'ESRCH') throw error;
    }
  };
  // The group can still contain children after the leader exits.
  signal('SIGTERM');
  // Allow descendants to close their own servers after the leader exits.
  // Immediately killing the group again also races with macOS reaping it.
  const deadline = Date.now() + 1800;
  while (Date.now() < deadline) {
    try {
      process.kill(-record.child.pid, 0);
    } catch (error) {
      if (errorCode(error) === 'ESRCH') return;
      if (errorCode(error) !== 'EPERM') throw error;
    }
    await new Promise((resolve) => setTimeout(resolve, 100));
  }
  signal('SIGKILL');
  await record.done;
}
