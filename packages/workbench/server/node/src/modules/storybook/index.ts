/**
 * A Storybook Workbench runs itself.
 *
 * A Storybook implementation without a `url` is the Workbench server's: it
 * starts the project's installed `storybook dev --preview-only` when a page
 * first asks for a story, on a local socket instead of a port (see
 * socket-listen.cjs), and reaches it only through the implementation's
 * proxy. A Storybook the user runs, on any port, is never in the way. The
 * process restarts on the next request after it exits, and stops with the
 * server. See specs/adapters/storybook.md.
 */
import { spawn, type ChildProcess } from 'node:child_process';
import fs from 'node:fs';
import http from 'node:http';
import { createRequire } from 'node:module';
import os from 'node:os';
import path from 'node:path';

const PRELOAD = path.join(import.meta.dirname, 'socket-listen.cjs');
const FLAGS = ['--ci', '--no-open', '--quiet', '--no-version-updates', '--disable-telemetry'];

export interface OwnedStorybookOptions {
  /** The folder Storybook runs in: the one with `.storybook/`. */
  root: string;
  /** How long a start may take before it is reported, in milliseconds. */
  timeout?: number;
  log?: (event: string, details: Record<string, unknown>) => void;
}

export interface OwnedStorybook {
  /** The socket the running Storybook answers on, starting it when needed. */
  socket(port: number): Promise<string>;
  /** Stops the Storybook and removes its socket folder. */
  close(): Promise<void>;
}

/** The project's Storybook command line script, from its own installation. */
export function storybookBin(root: string): string {
  let manifest: string;
  try {
    manifest = createRequire(path.join(root, 'package.json')).resolve('storybook/package.json');
  } catch {
    throw new Error('Storybook isn’t installed in ' + root + '. Install storybook and a framework there, or set url to a Storybook you run.');
  }
  const bin = JSON.parse(fs.readFileSync(manifest, 'utf8')).bin;
  const file = typeof bin === 'string' ? bin : bin && (bin.storybook || bin.sb);
  if (!file) throw new Error('The installed storybook package has no command line script.');
  return path.resolve(path.dirname(manifest), file);
}

/** Answers once the Storybook on `socket` serves its story index. */
function answers(socket: string): Promise<boolean> {
  return new Promise(resolve => {
    const req = http.get({ socketPath: socket, path: '/index.json', headers: { host: 'localhost' }, timeout: 2000 }, res => {
      res.resume();
      resolve(res.statusCode === 200);
    });
    req.on('error', () => resolve(false));
    req.on('timeout', () => { req.destroy(); resolve(false); });
  });
}

export function ownedStorybook(options: OwnedStorybookOptions): OwnedStorybook {
  const timeout = options.timeout ?? 120000;
  const log = options.log ?? (() => {});
  let starting: Promise<string> | null = null;
  let child: ChildProcess | null = null;
  let dir: string | null = null;
  let closed = false;

  function cleanup(): void {
    if (dir) fs.rmSync(dir, { recursive: true, force: true });
    dir = null;
  }

  function launch(port: number, previewOnly: boolean): Promise<string> {
    const bin = storybookBin(options.root);
    cleanup();
    /* Short, so a socket path stays under the platform's length limit. */
    dir = fs.mkdtempSync(path.join(os.tmpdir(), 'wb-sb-'));
    const own = dir;
    const args = ['--require', PRELOAD, bin, 'dev', '-p', String(port), '--exact-port', ...(previewOnly ? ['--preview-only'] : []), ...FLAGS];
    const env: NodeJS.ProcessEnv = { ...process.env, WORKBENCH_STORYBOOK_SOCKETS: own };
    /* The bundled Electron runs scripts as Node only when told to. */
    if (process.versions.electron) env.ELECTRON_RUN_AS_NODE = '1';
    const started = Date.now();
    const proc = spawn(process.execPath, args, { cwd: options.root, env, stdio: ['ignore', 'pipe', 'pipe'] });
    child = proc;
    let output = '';
    const keep = (chunk: Buffer) => { output = (output + chunk.toString()).slice(-4000); };
    proc.stdout?.on('data', keep);
    proc.stderr?.on('data', keep);
    log('storybook.starting', { root: options.root, port, previewOnly });

    return new Promise<string>((resolve, reject) => {
      let done = false;
      const finish = (error: Error | null, socket?: string) => {
        if (done) return;
        done = true;
        clearInterval(poll);
        if (error) reject(error); else resolve(socket!);
      };
      proc.on('exit', code => {
        if (child === proc) child = null;
        if (dir === own) cleanup();
        log('storybook.exited', { code, root: options.root });
        /* A Storybook that was running starts again on the next request. */
        if (done) { starting = null; return; }
        /* Storybook versions before --preview-only reject the flag. */
        if (previewOnly && /preview-only/.test(output) && /unknown option|unknown argument/i.test(output)) {
          launch(port, false).then(socket => finish(null, socket), error => finish(error));
          return;
        }
        finish(new Error('Storybook stopped before it was ready (exit ' + code + '):\n' + output.trim().slice(-1500)));
      });
      proc.on('error', error => finish(error));
      const poll = setInterval(() => {
        if (Date.now() - started > timeout) {
          proc.kill();
          finish(new Error('Storybook didn’t become ready within ' + Math.round(timeout / 1000) + 's:\n' + output.trim().slice(-1500)));
          return;
        }
        let socket: string;
        try { socket = fs.readFileSync(path.join(own, 'server'), 'utf8'); } catch { return; }
        answers(socket).then(ready => {
          if (!ready) return;
          log('storybook.ready', { root: options.root, ms: Date.now() - started });
          finish(null, socket);
        });
      }, 150);
    });
  }

  return {
    socket(port: number): Promise<string> {
      if (closed) return Promise.reject(new Error('Workbench has stopped this Storybook'));
      if (!starting) {
        starting = launch(port, true);
        starting.catch(() => { starting = null; });
      }
      return starting;
    },
    close(): Promise<void> {
      closed = true;
      const proc = child;
      child = null;
      starting = null;
      if (!proc || proc.exitCode !== null) { cleanup(); return Promise.resolve(); }
      return new Promise<void>(resolve => {
        const force = setTimeout(() => proc.kill('SIGKILL'), 3000);
        proc.once('exit', () => { clearTimeout(force); cleanup(); resolve(); });
        proc.kill('SIGTERM');
      });
    },
  };
}
