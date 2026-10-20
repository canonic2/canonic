/**
 * The host of one space's Workbench server.
 *
 * `workbench-server serve` owns the space's HTTP port and everything served
 * on it. It runs as this process's child and asks it, one JSON message per
 * line over stdio, for what belongs to the editor or the capture helper: the
 * Node runtime project code runs on, screenshots, opening a file, handoffs,
 * the other spaces, and native window streams. The extension and the
 * standalone command both start spaces through `start`.
 *
 * The server exits when its stdin closes, so it never outlives this process.
 * It runs in its own process group, killed when it exits, so its children
 * cannot outlive it either. A server that stops unexpectedly is started again
 * on the same port, at most three times a minute.
 */
import { spawn } from 'node:child_process';
import type { ChildProcessWithoutNullStreams } from 'node:child_process';
import crypto from 'node:crypto';
import { existsSync, promises as fs, mkdtempSync, rmSync } from 'node:fs';
import http from 'node:http';
import { createRequire } from 'node:module';
import os from 'node:os';
import path from 'node:path';
import { createInterface } from 'node:readline';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const product = fileURLToPath(new URL('../../../../..', import.meta.url));
const electronCapture = require('../../../../../capture/electron-capture');
const indexRuntime = require('../../../../../capture/electron-runtime');
const windowStream = require('../../../../../capture/window-stream');
const nativePool = require('../../../../../capture/src/native-streams/pool.ts');
const binary = require('../../../binary');

/** Ports to try in order. A stable one keeps links working between sessions; if all are taken the OS picks. */
export const PORTS = [3579, 3580, 3581, 3582, 3583, 0];

/* The server's routes, for the editor's and the scripts' own requests. */
export const CONFIG_PATH = '/_workbench/config';
export const CONFIG_FILE_PATH = '/_workbench/config-file';
export const SIZES_PATH = '/_workbench/sizes';
export const LOG_PATH = '/_workbench/log';
export const OPEN_PATH = '/_workbench/open';
export const HANDOFF_PATH = '/_workbench/handoff';
export const SHOT_PATH = '/_workbench/shot';
export const STORIES_PATH = '/_workbench/stories';
export const SPACES_PATH = '/_workbench/spaces';
export const SPACES_OPEN_PATH = '/_workbench/spaces/open';
export const EXPORT_PATH = '/_workbench/export';
export const CAPTURE_PATH = '/_workbench/capture';
export const CAPTURE_IMAGE_PATH = '/_workbench/capture/image';
export const CAPTURE_PREPARE_PATH = '/_workbench/capture/prepare';
export const CAPTURE_WARM_PATH = '/_workbench/capture/warm';
export const CAPTURE_PAGE_PATH = '/_workbench/capture/page';
export const CAPTURE_PAGE_IMAGE_PATH = '/_workbench/capture/page/image';
export const CAPTURE_PAGE_PREPARE_PATH = '/_workbench/capture/page/prepare';
export const SIMULATOR_PATH = '/_workbench/simulator';
export const SIMULATOR_INPUT_PATH = '/_workbench/simulator/input';
export const SIMULATOR_STREAM_PATH = '/_workbench/simulator/stream';
export const WINDOW_STREAM_PATH = '/_workbench/window/stream';
const STARTUP_MS = 10000;
const CLOSE_MS = 3000;
const RESTARTS = 3;
const RESTART_WINDOW_MS = 60000;

type Record = { level: string; event: string; details: { [key: string]: unknown } };
type Capture = { [method: string]: any };

export type HostOptions = {
  root: string;
  /** The folder of the space's workbench.yaml and its key there; the root's first space by default. */
  config?: { dir?: string; key?: string | null };
  isTrusted?: boolean;
  /** Warm the capture helper and build every docs page at startup. */
  eagerCapture?: boolean;
  eagerPreviews?: boolean;
  /** A capture service to use, shared with other spaces when `captureShared`. */
  capture?: Capture;
  captureShared?: boolean;
  captureStorage?: string;
  spaces?: { list(): unknown; open(id: string): unknown } | null;
  windowStream?: any;
  screenCapturePermissionOwner?: string;
  onLog?: (record: Record) => void;
  onShot?: (file: string) => void;
  onOpen?: (file: string) => unknown;
  onHandoff?: (prompt: string, canvas: unknown) => unknown;
  onCatalogChanged?: () => void;
  onPagesChanged?: () => void;
  /** Where the server binary is; the one this product ships by default. */
  binary?: string;
  /** Limits for the server's own policies, for tests. */
  logLimit?: number;
  exportRetentionMs?: number;
  exportMaxArchiveBytes?: number;
  viewStaleMs?: number;
  now?: () => number;
};

export type Running = {
  port: number;
  url: string;
  /** The space's configuration with every page this machine adds. */
  config: () => Promise<unknown>;
  /** Points `.canonic/.workbench/server.json` at this server again. */
  announce: () => void;
  close: () => Promise<void>;
};

/** The binary this product ships for this platform. */
export function binaryPath(): string {
  return binary.binaryPath();
}

/** What runs in every page the capture helper opens: the element-describing helper and its DevTools entry point. */
function describeSource(): string {
  return require('node:fs').readFileSync(path.join(product, 'ui/legacy/describe.js'), 'utf8') +
    '\nwindow.__wbDescribeAt = function (x, y) { return window.wbDescribe.at(document, x, y); };\n';
}

/** The screenshot service a host makes when it isn't given one; spaces can share one. */
export function createCapture(storage?: string, diagnostic?: (level: string, event: string, details: { [key: string]: unknown }) => void): Capture {
  return electronCapture.createService({ inject: describeSource(), storage, diagnostic });
}

/* A capture result's bytes, whatever shape the service gave them in. */
function shotOf(result: any): { body: Buffer | null; targets: unknown; details: unknown } {
  if (!result) return { body: null, targets: null, details: null };
  if (Buffer.isBuffer(result)) {
    const details = (result as any).captureDetails;
    return { body: result, targets: details && details.targets || null, details: details || null };
  }
  const body = result.pdf || result.image || result.png || null;
  return { body, targets: result.targets || (result.captureDetails && result.captureDetails.targets) || null, details: result.details || result.captureDetails || null };
}

export function start(options: HostOptions): Promise<Running> {
  const root = path.resolve(options.root);
  const where = { dir: path.resolve(options.config?.dir || root), key: options.config?.key || null };
  const binary = options.binary || binaryPath();
  const onLog = options.onLog || (() => {});
  function log(level: string, event: string, details: { [key: string]: unknown }) {
    try { onLog({ level, event, details }); } catch { /* Diagnostics never break the workbench. */ }
  }
  const capture: Capture = options.capture || createCapture(options.captureStorage, log);
  let captureReady: Promise<unknown> = Promise.resolve();
  const workers = new Map<string, Capture>();
  const pools = new Map<string, { close(): Promise<unknown> }>();
  const shots = mkdtempSync(path.join(os.tmpdir(), 'canonic-workbench-shots-'));
  const windowVideo = options.windowStream || nativePool.createPool(() => windowStream.create({
    storage: path.join(options.captureStorage || path.join(os.tmpdir(), 'canonic-workbench-capture'), 'window-capture'),
    path: WINDOW_STREAM_PATH,
    permissionOwner: options.screenCapturePermissionOwner,
  }));
  let streamSocket: Promise<string> | null = null;
  let streamServer: http.Server | null = null;

  /* Window streams are the capture part's: the server forwards their
     requests to this socket, which only it can reach. */
  function streams(): Promise<string> {
    if (streamSocket) return streamSocket;
    const folder = mkdtempSync(path.join(os.tmpdir(), 'wb-streams-'));
    const socket = process.platform === 'win32' ? '\\\\.\\pipe\\workbench-streams-' + crypto.randomBytes(8).toString('hex') : path.join(folder, 'streams.sock');
    streamServer = http.createServer((req, res) => {
      if (!windowVideo.acceptHttp || !windowVideo.acceptHttp(req, res)) {
        res.writeHead(403, { 'Content-Type': 'text/plain; charset=utf-8' });
        res.end('That window stream is not active.');
      }
    });
    streamServer.on('upgrade', (req, sock) => { if (!windowVideo.accept(req, sock)) sock.destroy(); });
    streamServer.on('close', () => rmSync(folder, { recursive: true, force: true }));
    const server = streamServer;
    streamSocket = new Promise((resolve, reject) => {
      server.once('error', reject);
      server.listen(socket, () => resolve(socket));
    });
    return streamSocket;
  }

  let node: Promise<string> | null = null;
  function runtime(): Promise<string> {
    if (!node) {
      node = indexRuntime.available()
        ? indexRuntime.prepare(options.captureStorage, null, (event: string, details: { [key: string]: unknown }) => log('info', event, { for: 'page-index', ...details }))
        : Promise.resolve(process.execPath);
      node!.catch(() => { node = null; });
    }
    return node!;
  }

  async function captured(operation: string, base: string, payload: unknown, worker: string | null): Promise<unknown> {
    await captureReady.catch(() => {});
    const service = worker ? workers.get(worker) : capture;
    if (!service) throw new Error('That capture worker is closed.');
    const method = service[operation];
    if (typeof method !== 'function') throw new Error('The capture service can’t ' + operation + '.');
    const result = operation === 'warm' ? await method.call(service, base)
      : operation === 'prepare' || operation === 'capture' ? await method.call(service, base, payload)
        : await method.call(service, payload);
    const shot = shotOf(result);
    if (!shot.body) return { file: null, targets: shot.targets, details: shot.details };
    const file = path.join(shots, crypto.randomBytes(12).toString('hex'));
    await fs.writeFile(file, shot.body);
    return { file, targets: shot.targets, details: shot.details };
  }

  async function answer(question: string, message: { [key: string]: any }): Promise<unknown> {
    switch (question) {
      case 'node': return runtime();
      case 'capture': return captured(String(message.operation), String(message.base || ''), message.payload, message.worker || null);
      case 'capture-pool': {
        await captureReady.catch(() => {});
        const pool = capture.createPool && Number(message.size) > 1 ? capture.createPool(Number(message.size)) : { workers: [capture], close: () => Promise.resolve() };
        const id = crypto.randomBytes(8).toString('hex');
        pools.set(id, pool);
        const names = pool.workers.map((worker: Capture, at: number) => { const name = id + ':' + at; workers.set(name, worker); return name; });
        return { workers: names };
      }
      case 'capture-pool-close': {
        const names: string[] = Array.isArray(message.workers) ? message.workers : [];
        names.forEach(name => workers.delete(name));
        const id = String(names[0] || '').split(':')[0];
        const pool = pools.get(id);
        pools.delete(id);
        return pool ? pool.close() : null;
      }
      case 'open':
        if (!options.onOpen) throw new Error('nothing is set up to open files');
        return Promise.resolve(options.onOpen(String(message.file))).then(() => null);
      case 'handoff':
        if (!options.onHandoff) throw new Error('nothing is set up to receive a handoff');
        return Promise.resolve(options.onHandoff(String(message.prompt), message.canvas)).then(() => null);
      case 'spaces': return options.spaces ? options.spaces.list() : null;
      case 'space-open':
        if (!options.spaces) throw new Error('That space isn’t in the Workbench list.');
        return options.spaces.open(String(message.space));
      case 'stream':
        if (message.operation === 'stop') return Promise.resolve(windowVideo.stop(message.request && message.request.id)).then(() => null);
        return windowVideo.start(message.request);
      case 'stream-socket': return streams();
      default: throw new Error('The host can’t answer ' + question + '.');
    }
  }

  let child: ChildProcessWithoutNullStreams | null = null;
  let port = 0;
  let stopping = false;
  const restarts: number[] = [];
  const pending = new Map<number, { resolve: (value: unknown) => void; reject: (error: Error) => void }>();
  let next = 1;

  function args(ports: number[]): string[] {
    const out = ['serve', '--ports', ports.join(','), '--root', root, '--product', product, '--space-dir', where.dir];
    if (where.key) out.push('--space-key', where.key);
    if (options.isTrusted === false) out.push('--untrusted');
    if (options.onHandoff) out.push('--handoff');
    if (options.eagerPreviews) out.push('--eager-previews');
    if (options.logLimit) out.push('--log-limit', String(options.logLimit));
    if (options.exportRetentionMs) out.push('--export-retention-ms', String(options.exportRetentionMs));
    if (options.exportMaxArchiveBytes) out.push('--export-max-archive-bytes', String(options.exportMaxArchiveBytes));
    if (options.viewStaleMs) out.push('--view-stale-ms', String(options.viewStaleMs));
    return out;
  }

  function launch(ports: number[]): Promise<number> {
    const started = spawn(binary, args(ports), { stdio: ['pipe', 'pipe', 'pipe'], detached: process.platform !== 'win32' });
    child = started;
    let stderr = '';
    started.stderr.on('data', chunk => { if (stderr.length < 4000) stderr += String(chunk).slice(0, 4000 - stderr.length); });
    started.stdin.on('error', () => { /* A closed input is reported by the exit. */ });
    function killGroup() {
      if (process.platform === 'win32' || !started.pid) { started.kill('SIGKILL'); return; }
      try { process.kill(-started.pid, 'SIGKILL'); } catch { /* The group is already gone. */ }
    }
    function write(message: unknown) {
      if (started.exitCode === null && !started.stdin.destroyed) started.stdin.write(JSON.stringify(message) + '\n');
    }
    return new Promise((resolve, reject) => {
      let listening = false;
      const timer = setTimeout(() => fail(new Error('The Workbench server did not start within ' + STARTUP_MS / 1000 + ' seconds.')), STARTUP_MS);
      function fail(error: Error) {
        clearTimeout(timer);
        if (listening) return;
        listening = true;
        killGroup();
        reject(error);
      }
      started.on('error', error => fail(error));
      started.on('exit', (code, signal) => {
        killGroup();
        const error = new Error('The Workbench server stopped (' + (signal || 'exit ' + code) + ')' + (stderr ? ': ' + stderr.trim() : '.'));
        if (child === started) {
          for (const waiting of pending.values()) waiting.reject(error);
          pending.clear();
        }
        if (!listening) { fail(error); return; }
        if (stopping || child !== started) return;
        log('error', 'server.exited', { message: error.message });
        const now = Date.now();
        while (restarts.length && now - restarts[0]! >= RESTART_WINDOW_MS) restarts.shift();
        if (restarts.length >= RESTARTS) { log('error', 'server.stopped', { restarts: restarts.length }); return; }
        restarts.push(now);
        launch([port]).catch(again => log('error', 'server.restart.failed', { message: String(again.message || again) }));
      });
      createInterface({ input: started.stdout }).on('line', line => {
        let message: { [key: string]: any };
        try { message = JSON.parse(line); } catch { log('warn', 'server.message.invalid', { line: line.slice(0, 200) }); return; }
        switch (message.type) {
          case 'listening':
            if (!listening && typeof message.port === 'number') { listening = true; clearTimeout(timer); resolve(message.port); }
            return;
          case 'host':
            Promise.resolve().then(() => answer(String(message.question), message)).then(
              value => write({ type: 'reply', id: message.id, ok: true, value: value === undefined ? null : value }),
              error => write({ type: 'reply', id: message.id, ok: false, error: String(error && error.message || error) }));
            return;
          case 'answer': {
            const waiting = pending.get(message.id);
            pending.delete(message.id);
            if (!waiting) return;
            if (message.ok) waiting.resolve(message.response);
            else waiting.reject(Object.assign(new Error(message.error || 'The Workbench server failed.'), { status: message.status }));
            return;
          }
          case 'log': if (message.record) { try { onLog(message.record); } catch { /* as above */ } } return;
          case 'diagnostic': log('info', String(message.event), message.details || {}); return;
          case 'shot': if (options.onShot) { try { options.onShot(String(message.file)); } catch { /* as above */ } } return;
          case 'changed': if (options.onPagesChanged) { try { options.onPagesChanged(); } catch (error) { log('warn', 'pages.notify.failed', { message: String((error as Error).message || error) }); } } return;
          case 'catalog': if (options.onCatalogChanged) { try { options.onCatalogChanged(); } catch (error) { log('warn', 'catalog.notify.failed', { message: String((error as Error).message || error) }); } } return;
          case 'error': log('warn', 'server.error', { message: message.error }); return;
          default: log('warn', 'server.message.unknown', { type: String(message.type) });
        }
      });
    }).then(listened => {
      port = listened as number;
      log('info', 'server.started', { port });
      return port;
    });
  }

  function ask(message: { [key: string]: unknown }, timeoutMs: number): Promise<unknown> {
    const current = child;
    if (!current || current.exitCode !== null) return Promise.reject(new Error('The Workbench server is not running.'));
    const id = next++;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => { pending.delete(id); reject(new Error('The Workbench server did not answer within ' + timeoutMs / 1000 + ' seconds.')); }, timeoutMs);
      pending.set(id, {
        resolve: value => { clearTimeout(timer); resolve(value); },
        reject: error => { clearTimeout(timer); reject(error); },
      });
      current.stdin.write(JSON.stringify({ ...message, id }) + '\n');
    });
  }

  if (!existsSync(binary)) {
    return Promise.reject(new Error('Workbench needs its server binary, which isn’t at ' + binary + '.'));
  }
  return launch(PORTS).then(listened => {
    const base = 'http://127.0.0.1:' + listened + '/';
    if (options.eagerCapture) {
      captureReady = Promise.resolve().then(() => capture.warm(base)).catch(error => {
        log('warn', 'capture.warm.failed', { message: String(error.message || error) });
      });
    }
    return {
      port: listened,
      url: base + '_workbench/',
      config: () => ask({ type: 'catalog' }, 300000),
      announce: () => { ask({ type: 'announce' }, STARTUP_MS).catch(error => log('warn', 'view.announce.failed', { message: String(error.message || error) })); },
      close: () => {
        stopping = true;
        const current = child;
        const exited = !current || current.exitCode !== null || current.signalCode !== null ? Promise.resolve() : new Promise<void>(resolve => {
          const timer = setTimeout(() => { try { process.kill(-current.pid!, 'SIGKILL'); } catch { current.kill('SIGKILL'); } }, CLOSE_MS);
          current.once('exit', () => { clearTimeout(timer); resolve(); });
          current.stdin.end();
        });
        return exited.then(() => Promise.all([
          options.captureShared ? Promise.resolve() : Promise.resolve(capture.close()).catch(() => {}),
          Promise.resolve(windowVideo.close()).catch(() => {}),
          Promise.all(Array.from(pools.values()).map(pool => Promise.resolve(pool.close()).catch(() => {}))),
          streamServer ? new Promise(resolve => streamServer!.close(resolve)) : null,
        ])).then(() => { rmSync(shots, { recursive: true, force: true }); });
      },
    };
  });
}
