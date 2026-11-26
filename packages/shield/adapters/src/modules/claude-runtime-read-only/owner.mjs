import { spawn } from 'node:child_process';

/** Hold the native context lease through the owning SDK client's stdin pipe. */
export async function openOwner(binary, args, env, timeoutMs) {
  const child = spawn(binary, args, { shell: false, stdio: ['pipe', 'pipe', 'pipe'], env });
  const failed = () => Error('Shield SDK context owner failed');
  const lifetime = new AbortController();
  const exited = new Promise(resolve => child.once('close', (code, signal) => {
    lifetime.abort();
    resolve(code === 0 && signal === null);
  }));
  const kill = () => child.kill('SIGKILL');
  child.stdin.on('error', kill);
  let stderrBytes = 0;
  child.stderr.on('data', chunk => {
    stderrBytes += chunk.length;
    if (stderrBytes > 65_536) kill();
  });
  let token;
  try {
    token = await new Promise((resolve, reject) => {
      let pending = Buffer.alloc(0);
      const timer = setTimeout(() => reject(failed()), timeoutMs);
      child.once('error', () => { clearTimeout(timer); reject(failed()); });
      child.once('close', () => { clearTimeout(timer); reject(failed()); });
      child.stdout.on('data', chunk => {
        if (pending.length + chunk.length > 65) {
          clearTimeout(timer);
          kill();
          reject(failed());
          return;
        }
        pending = Buffer.concat([pending, chunk]);
        if (pending.length !== 65) return;
        clearTimeout(timer);
        const line = pending.toString('ascii');
        if (/^[a-f0-9]{64}\n$/.test(line) && Buffer.from(line).equals(pending)) {
          resolve(line.slice(0, 64));
        } else {
          reject(failed());
        }
      });
    });
  } catch (error) {
    kill();
    await exited;
    throw error;
  }
  child.unref();
  child.stdin.unref();
  child.stdout.unref();
  child.stderr.unref();
  return {
    token,
    signal: lifetime.signal,
    async close() {
      child.stdin.end();
      const timer = setTimeout(kill, timeoutMs);
      try {
        if (!await exited) throw failed();
      } finally {
        clearTimeout(timer);
      }
    },
  };
}
