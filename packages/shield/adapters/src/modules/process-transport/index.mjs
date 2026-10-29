import { spawn } from 'node:child_process';

const LIMITS = Object.freeze({ stdout: 4_194_304, stderr: 65_536 });

/** Bounded direct-child transport failure, with no native diagnostics or payload. */
class ProcessTransportError extends Error {
  constructor(code) { super('Shield process transport failed'); this.name = 'ProcessTransportError'; this.code = code; }
}

/** Owns one direct child; it does not launch host tools or authorize operations. */
export async function transport(captured, validate) {
  const { binary, args, timeoutMs, signal, input, expected } = captured;
  if (signal?.aborted) throw new ProcessTransportError('cancelled');
  return new Promise((resolve, reject) => {
    let child;
    try { child = spawn(binary, args, { shell: false, stdio: ['pipe', 'pipe', 'pipe'], windowsHide: true }); }
    catch { reject(new ProcessTransportError('spawn-failed')); return; }
    let failure;
    let finished = false;
    let stdoutSize = 0;
    let stderrSize = 0;
    const chunks = [];
    const timers = new Set();
    const timer = (fn, ms) => { const handle = setTimeout(fn, ms); timers.add(handle); };
    const finish = (error, value) => {
      if (finished) return;
      finished = true;
      for (const handle of timers) clearTimeout(handle);
      signal?.removeEventListener('abort', abort);
      if (error) reject(new ProcessTransportError(error)); else resolve(value);
    };
    const fail = code => {
      if (failure || finished) return;
      failure = code;
      child.stdin.destroy(); child.stdout.destroy(); child.stderr.destroy();
      child.kill('SIGTERM');
      timer(() => {
        child.kill('SIGKILL');
        timer(() => { child.unref(); finish('termination-failed'); }, 250);
      }, 250);
    };
    const abort = () => fail('cancelled');
    child.on('error', () => fail('spawn-failed'));
    child.stdin.on('error', () => fail('input-write'));
    child.stdout.on('error', () => fail('invalid-response'));
    child.stderr.on('error', () => fail('invalid-response'));
    child.stdout.on('data', chunk => {
      stdoutSize += chunk.length;
      if (stdoutSize > LIMITS.stdout) fail('output-limit'); else chunks.push(chunk);
    });
    child.stderr.on('data', chunk => { stderrSize += chunk.length; if (stderrSize > LIMITS.stderr) fail('output-limit'); });
    child.on('close', (code, terminationSignal) => {
      if (failure) { finish(failure); return; }
      if (terminationSignal || ![0, 2].includes(code)) { finish('process-exit'); return; }
      try { finish(null, validate(Buffer.concat(chunks), code, expected)); }
      catch { finish('invalid-response'); }
    });
    signal?.addEventListener('abort', abort, { once: true });
    timer(() => fail('timeout'), timeoutMs);
    if (signal?.aborted) abort(); else child.stdin.end(input);
  });
}
