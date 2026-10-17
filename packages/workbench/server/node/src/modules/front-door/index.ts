/**
 * The Rust front door: `workbench-server serve` owns the space's public
 * loopback port and forwards every request to this Node server's private one.
 * It also keeps the page index resident and reports when the page tree
 * changed on disk. Messages are one JSON object per line on its stdio; it
 * exits when its stdin closes, so it never outlives this process. It runs in
 * its own process group, which is killed when it exits, so an evaluator it
 * started cannot outlive it either.
 */
import { spawn } from 'node:child_process';
import type { ChildProcessWithoutNullStreams } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';

const extension = fileURLToPath(new URL('../../../../..', import.meta.url));
const STARTUP_MS = 10000;
const CLOSE_MS = 3000;

type Diagnostic = (level: string, event: string, details: { [key: string]: unknown }) => void;

export type StartOptions = {
  /** The Node server's private port. */
  upstream: number;
  /** Public ports to try in order; 0 takes any free port. */
  ports: number[];
  /** The page tree changed on disk. */
  onChanged: () => void;
  /** The front door stopped without being asked to. */
  onExit: (error: Error) => void;
  diagnostic: Diagnostic;
  binary?: string;
};

export type FrontDoor = {
  port: number;
  /** One page-index request, answered by the resident index within `timeoutMs`. */
  index: (request: unknown, timeoutMs: number) => Promise<unknown>;
  close: () => Promise<void>;
};

/** The binary this extension ships for its platform. */
export function binaryPath(): string {
  return path.join(extension, 'server/bin', process.platform + '-' + process.arch, 'workbench-server' + (process.platform === 'win32' ? '.exe' : ''));
}

export function available(binary = binaryPath()): boolean {
  return existsSync(binary);
}

export function start(options: StartOptions): Promise<FrontDoor> {
  const binary = options.binary || binaryPath();
  const child: ChildProcessWithoutNullStreams = spawn(binary, ['serve', '--upstream', String(options.upstream), '--ports', options.ports.join(',')], { stdio: ['pipe', 'pipe', 'pipe'], detached: process.platform !== 'win32' });
  const pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void }>();
  let next = 1;
  let closing = false;
  let stderr = '';
  child.stderr.on('data', chunk => { if (stderr.length < 4000) stderr += String(chunk).slice(0, 4000 - stderr.length); });
  child.stdin.on('error', () => { /* A closed input is reported by the exit. */ });
  /* The whole group: the front door and any evaluator it left running. */
  function killGroup() {
    if (process.platform === 'win32' || !child.pid) { child.kill('SIGKILL'); return; }
    try { process.kill(-child.pid, 'SIGKILL'); } catch { /* The group is already gone. */ }
  }

  return new Promise((resolve, reject) => {
    let started = false;
    const timer = setTimeout(() => fail(new Error('The Workbench front door did not start within ' + STARTUP_MS / 1000 + ' seconds.')), STARTUP_MS);
    function fail(error: Error) {
      clearTimeout(timer);
      if (started) return;
      started = true;
      closing = true;
      killGroup();
      reject(error);
    }
    child.on('error', error => fail(error));
    child.on('exit', (code, signal) => {
      killGroup();
      const error = new Error('The Workbench front door stopped (' + (signal || 'exit ' + code) + ')' + (stderr ? ': ' + stderr.trim() : '.'));
      for (const waiting of pending.values()) waiting.reject(error);
      pending.clear();
      if (!started) fail(error);
      else if (!closing) options.onExit(error);
    });
    createInterface({ input: child.stdout }).on('line', line => {
      let message: { type?: string; port?: number; id?: number; ok?: boolean; response?: unknown; error?: string };
      try { message = JSON.parse(line); } catch {
        options.diagnostic('warn', 'front-door.message.invalid', { line: line.slice(0, 200) });
        return;
      }
      if (message.type === 'listening' && typeof message.port === 'number' && !started) {
        started = true;
        clearTimeout(timer);
        resolve({ port: message.port, index, close });
      } else if (message.type === 'answer' && typeof message.id === 'number') {
        const waiting = pending.get(message.id);
        pending.delete(message.id);
        if (!waiting) return;
        if (message.ok) waiting.resolve(message.response);
        else waiting.reject(new Error(message.error || 'The resident page index failed.'));
      } else if (message.type === 'changed') {
        options.onChanged();
      } else if (message.type === 'error') {
        options.diagnostic('warn', 'front-door.error', { message: message.error });
      }
    });
  });

  function index(request: unknown, timeoutMs: number): Promise<unknown> {
    if (closing || child.exitCode !== null) return Promise.reject(new Error('The Workbench front door is not running.'));
    const id = next++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        pending.delete(id);
        reject(new Error('The resident page index did not answer within ' + timeoutMs / 1000 + ' seconds.'));
      }, timeoutMs);
      pending.set(id, {
        resolve: value => { clearTimeout(timer); resolve(value); },
        reject: error => { clearTimeout(timer); reject(error); },
      });
      child.stdin.write(JSON.stringify({ type: 'index', id, request }) + '\n');
    });
  }

  function close(): Promise<void> {
    closing = true;
    if (child.exitCode !== null || child.signalCode !== null) return Promise.resolve();
    return new Promise(resolve => {
      const timer = setTimeout(killGroup, CLOSE_MS);
      child.once('exit', () => { clearTimeout(timer); resolve(); });
      child.stdin.end();
    });
  }
}
