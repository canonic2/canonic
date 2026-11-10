// A retained native leader, owned by the controller, anchors every group signal.
import { spawn } from 'node:child_process';
import { fileURLToPath } from 'node:url';

const controller = fileURLToPath(new URL('./native-command.py', import.meta.url));

export function command(program, args, { cwd = process.cwd(), env = {}, timeoutMs = 40_000, abortSignal } = {}) {
  return new Promise((resolve, reject) => {
    if (abortSignal?.aborted) { reject(Error('Owned native fixture cancelled')); return; }
    if (!Number.isSafeInteger(timeoutMs) || timeoutMs <= 0) { reject(Error('Invalid native fixture deadline')); return; }
    const input = JSON.stringify({ program, args, cwd, env, timeout_ms: timeoutMs });
    if (Buffer.byteLength(input) > 1024 * 1024) { reject(Error('Owned native fixture launch exceeded its bound')); return; }
    const child = spawn('python3', [controller], { env: { PATH: process.env.PATH ?? '/usr/bin:/bin' },
      shell: false, stdio: ['pipe', 'pipe', 'pipe', 'pipe'] });
    const chunks = { stdout: [], stderr: [] }, status = [];
    let size = 0, statusSize = 0, failure, settled = false, drainTimer;
    const finish = (error, result) => {
      if (settled) return;
      settled = true;
      clearTimeout(deadline);
      clearTimeout(drainTimer);
      abortSignal?.removeEventListener('abort', abort);
      if (error) reject(error); else resolve(result);
    };
    const stop = error => {
      if (failure || settled) return;
      failure = error;
      // EOF also requests cleanup, including if this parent itself exits.
      child.stdin.end();
    };
    const abort = () => stop(Error('Owned native fixture cancelled'));
    const deadline = setTimeout(() => stop(Error('Owned native host exceeded its deadline')), timeoutMs);
    abortSignal?.addEventListener('abort', abort, { once: true });
    child.stdin.on('error', () => {}); // The controller can close after target spawn failure.
    child.stdin.write(`${input}\n`);
    if (abortSignal?.aborted) abort();
    for (const stream of ['stdout', 'stderr']) child[stream].on('data', bytes => {
      size += bytes.length;
      if (size > 2 * 1024 * 1024) stop(Error('Owned native host output exceeded its bound'));
      else chunks[stream].push(bytes);
    });
    child.stdout.on('error', stop);
    child.stderr.on('error', stop);
    child.stdio[3].on('data', bytes => {
      statusSize += bytes.length;
      if (statusSize > 8192) stop(Error('Owned native controller exceeded its status bound'));
      else status.push(bytes);
    });
    child.stdio[3].on('error', stop);
    child.on('error', error => finish(error));
    child.on('exit', () => {
      // A process outside the promised group could still hold inherited pipes.
      // Bound draining after the owned controller is reaped; do not chase PIDs.
      drainTimer = setTimeout(() => {
        failure ??= Error('Owned native output remained open after controller cleanup');
        child.stdout.destroy(); child.stderr.destroy(); child.stdio[3].destroy();
      }, 250);
    });
    child.on('close', (code, signal) => {
      // Node has reaped only the controller. It NEVER signals a numeric group.
      let result;
      try {
        result = JSON.parse(Buffer.concat(status).toString('utf8'));
        if (code !== 0 || signal || result.reaped !== true || result.error) {
          const stage = ['input', 'capability', 'spawn', 'observe', 'cleanup'].includes(result.stage) ? result.stage : 'unspecified';
          const cleanup = ['term', 'kill', 'reap'].includes(result.cleanup_stage) ? result.cleanup_stage : 'none';
          const errno = Number.isInteger(result.errno) && result.errno > 0 && result.errno < 4096 ? result.errno : 'none';
          throw Error(`Owned native controller did not complete cleanup (stage=${stage}, cleanup=${cleanup}, errno=${errno}, controller_exit=${code}, controller_signal=${signal})`);
        }
      } catch (error) { finish(failure ?? error); return; }
      finish(failure, { code: result.code, signal: result.signal,
        stdout: Buffer.concat(chunks.stdout).toString('utf8'), stderr: Buffer.concat(chunks.stderr).toString('utf8') });
    });
  });
}
