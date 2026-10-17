// Page-index process, saved storage, deadline and shipping-path fallback owner.
import { spawn } from 'node:child_process';
import { createHash, randomUUID } from 'node:crypto';
import { promises as fs } from 'node:fs';
import os from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { readAnswer } from './answer.ts';
import type { IndexAnswer } from './answer.ts';
export { discoveryOrder, readAnswer } from './answer.ts';

const extension = fileURLToPath(new URL("../../../../..", import.meta.url));
const evaluator = path.join(extension, 'server/node/preview/evaluate-definitions.cjs');
const digest = (value: string) => createHash('sha256').update(value).digest('hex');
type Diagnostic = (level: string, event: string, details: { [key: string]: unknown }) => void;
export type IndexOptions = {
  root: string;
  previews?: { [key: string]: unknown };
  executable: () => Promise<string>;
  fallback: () => Promise<IndexAnswer>;
  diagnostic: Diagnostic;
  binary?: string;
  timeoutMs?: number;
  /** The front door's resident index; a process per request when absent or failing. */
  resident?: (request: unknown, timeoutMs: number) => Promise<unknown>;
};

async function identity(binary: string, options: unknown): Promise<string> {
  const files = [binary, evaluator, path.join(extension, 'server/node/preview/compiler.cjs'), path.join(extension, 'sdk/api.js')];
  const stamps = await Promise.all(files.map(async file => { const stat = await fs.stat(file); return [file, stat.mtimeMs, stat.size]; }));
  return digest(JSON.stringify([stamps, options]));
}

// At most 32 projects and 64 MiB per project; discard inactive projects
// after seven days. Eviction touches only this capability's temp directory.
async function savedPath(root: string): Promise<string> {
  const base = path.join(os.tmpdir(), 'canonic-workbench-index');
  const project = digest(path.resolve(root));
  await fs.mkdir(base, { recursive: true });
  const directories = await fs.readdir(base, { withFileTypes: true });
  const ages = await Promise.all(directories.filter(item => item.isDirectory() && /^[a-f0-9]{64}$/.test(item.name)).map(async item => {
    const file = path.join(base, item.name);
    try { return { file, name: item.name, modified: (await fs.stat(file)).mtimeMs }; }
    catch { return null; }
  }));
  const ordered = ages.filter(item => item !== null).filter(item => item.name !== project).sort((a, b) => b.modified - a.modified);
  await Promise.all(ordered.filter((item, at) => at >= 31 || Date.now() - item.modified > 7 * 86400000).map(item => fs.rm(item.file, { recursive: true, force: true })));
  const directory = path.join(base, project);
  await fs.mkdir(directory, { recursive: true });
  await fs.utimes(directory, new Date(), new Date());
  return path.join(directory, 'index.postcard');
}

function invoke(binary: string, request: unknown, timeoutMs: number): Promise<unknown> {
  return new Promise((resolve, reject) => {
    const child = spawn(binary, ['index'], { detached: process.platform !== 'win32', stdio: ['pipe', 'pipe', 'pipe'] });
    const output: Buffer[] = [];
    let bytes = 0;
    let stderr = '';
    let failure: Error | undefined;
    function kill(error: Error) {
      failure = error;
      if (!child.pid) return;
      if (process.platform === 'win32') {
        const killer = spawn('taskkill', ['/pid', String(child.pid), '/T', '/F'], { stdio: 'ignore' });
        killer.on('error', () => child.kill());
        killer.on('exit', code => { if (code !== 0) child.kill(); });
      } else {
        try { process.kill(-child.pid, 'SIGKILL'); } catch { child.kill('SIGKILL'); }
      }
    }
    const timer = setTimeout(() => kill(new Error('Native page index timed out')), timeoutMs);
    child.stdout.on('data', (chunk: Buffer) => {
      bytes += chunk.length;
      if (bytes > 128 * 1024 * 1024) kill(new Error('Native page index response exceeds 128 MiB'));
      else output.push(chunk);
    });
    child.stderr.on('data', chunk => { if (stderr.length < 4000) stderr += String(chunk).slice(0, 4000 - stderr.length); });
    child.stdin.on('error', error => { failure ??= error; });
    child.on('error', error => { failure ??= error; });
    child.on('close', code => {
      clearTimeout(timer);
      if (failure) return reject(failure);
      if (code !== 0) return reject(new Error('Native page index failed: ' + stderr));
      try { resolve(JSON.parse(Buffer.concat(output).toString())); } catch (error) { reject(error); }
    });
    child.stdin.end(JSON.stringify(request));
  });
}

export async function indexPages(options: IndexOptions): Promise<{ answer: IndexAnswer; timings: { [key: string]: unknown } }> {
  try {
    const binary = options.binary || path.join(extension, 'server/bin', process.platform + '-' + process.arch, 'workbench-server' + (process.platform === 'win32' ? '.exe' : ''));
    const saved = await savedPath(options.root);
    const key = await identity(binary, options.previews || {});
    const answerFile = path.join(path.dirname(saved), 'answer.json');
    let cached: { identity: string; generation: string; answer: IndexAnswer } | undefined;
    try {
      if ((await fs.stat(answerFile)).size <= 32 * 1024 * 1024) {
        const candidate = JSON.parse(await fs.readFile(answerFile, 'utf8'));
        if (candidate.identity === key && typeof candidate.generation === 'string' && Array.isArray(candidate.answer?.previews) && Array.isArray(candidate.answer?.docs) && Array.isArray(candidate.answer?.errors)) cached = candidate;
      }
    } catch { /* An absent or corrupt saved answer is rebuilt from native records. */ }
    // Runtime extraction is lazy: literal-only indexes never need Electron.
    // Native requests evaluation through this executable only when necessary.
    const timeoutMs = options.timeoutMs || 120000;
    // The evaluator's own deadline, which the resident index needs: no process
    // around it is killed at this one's deadline.
    const request = { root: path.resolve(options.root), saved, identity: key, evaluator, node: '', options: options.previews || {}, cached_generation: cached?.generation || '', deadline_ms: timeoutMs };
    const ask = async () => {
      if (options.resident) {
        try { return await options.resident(request, timeoutMs); } catch (error) {
          options.diagnostic('warn', 'previews.index.resident.failed', { message: String(error instanceof Error ? error.message : error) });
        }
      }
      return invoke(binary, request, timeoutMs);
    };
    let raw = await ask();
    let first: { [key: string]: unknown } | undefined;
    if (raw && typeof raw === 'object' && 'needs_evaluator' in raw && raw.needs_evaluator === true) {
      if ('timings' in raw && raw.timings && typeof raw.timings === 'object') first = raw.timings as { [key: string]: unknown };
      request.node = await options.executable();
      raw = await ask();
    }
    if (cached && raw && typeof raw === 'object' && 'reuse_answer' in raw && raw.reuse_answer === true && 'generation' in raw && raw.generation === cached.generation && 'timings' in raw && raw.timings && typeof raw.timings === 'object') return { answer: cached.answer, timings: raw.timings as { [key: string]: unknown } };
    const result = readAnswer(raw);
    if (first) {
      for (const [key, value] of Object.entries(first)) {
        if (typeof value === 'number' && (key.endsWith('Ms') || key === 'parsed' || key === 'directoriesRead')) result.timings[key] = value + Number(result.timings[key] || 0);
      }
      if ('reused' in first) result.timings.reused = first.reused;
      result.timings.passes = 2;
      if (first.saveError) result.timings.saveError = first.saveError;
    }
    if (raw && typeof raw === 'object' && 'generation' in raw && typeof raw.generation === 'string') {
      const bytes = JSON.stringify({ identity: key, generation: raw.generation, answer: result.answer });
      if (Buffer.byteLength(bytes) <= 32 * 1024 * 1024) {
        const temporary = answerFile + '.' + process.pid + '.' + randomUUID() + '.tmp';
        try { await fs.writeFile(temporary, bytes); await fs.rename(temporary, answerFile); }
        catch (error) { await fs.rm(temporary, { force: true }).catch(() => {}); options.diagnostic('warn', 'previews.index.save.failed', { message: String(error) }); }
      }
    }
    if (result.timings.saveError) options.diagnostic('warn', 'previews.index.save.failed', { message: result.timings.saveError });
    return result;
  } catch (error) {
    options.diagnostic('warn', 'previews.index.fallback', { message: String(error instanceof Error ? error.message : error) });
    return { answer: await options.fallback(), timings: { fallback: true } };
  }
}
