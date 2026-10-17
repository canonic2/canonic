/** Workbench's process adapter delegates mobile operations to the Rust product. */
import { execFile } from 'node:child_process';
import { existsSync } from 'node:fs';
import path from 'node:path';

export function create(root: string) {
  const executable = path.join(root, '.canonic/sandbox/bin/sandbox');
  function invoke(udid: string, args: string[]): Promise<Record<string, unknown>> {
    return new Promise((resolve, reject) => {
      if (!existsSync(executable)) { reject(new Error('Sandbox is not installed at .canonic/sandbox/bin/sandbox')); return; }
      execFile(executable, ['ios', '--id', `simulator:${udid}`, ...args], {
        cwd: root, encoding: 'utf8', maxBuffer: 16 * 1024 * 1024, timeout: 30000,
      }, (error, stdout, stderr) => {
        if (error) { reject(new Error(String(stderr || error.message).trim())); return; }
        try {
          const value = JSON.parse(stdout);
          if (!value || typeof value !== 'object' || Array.isArray(value)) throw new Error('Invalid object');
          resolve(value);
        } catch { reject(new Error('Sandbox returned invalid JSON')); }
      });
    });
  }
  return {
    async screen(udid: string) {
      const shot = await invoke(udid, ['screenshot']);
      if (typeof shot.sourceWidth !== 'number' || typeof shot.sourceHeight !== 'number' || shot.sourceWidth <= 0 || shot.sourceHeight <= 0) throw new Error('Sandbox screenshot omitted valid display dimensions');
      return { width: shot.sourceWidth, height: shot.sourceHeight };
    },
    async input(udid: string, request: Record<string, unknown>) {
      const ratio = (v: unknown) => typeof v === 'number' && Number.isFinite(v) && v >= 0 && v <= 1;
      if (request.action === 'tap' && ratio(request.x) && ratio(request.y)) {
        return invoke(udid, ['tap', String(request.x), String(request.y)]);
      }
      if (request.action === 'swipe' && [request.fromX, request.fromY, request.toX, request.toY].every(ratio)) {
        const duration = Math.round(Math.max(0.05, Math.min(Number(request.duration) || 0.2, 2)) * 1000);
        return invoke(udid, ['swipe', String(request.fromX), String(request.fromY), String(request.toX), String(request.toY), '--duration-ms', String(duration)]);
      }
      throw new Error('Invalid simulator action or normalized coordinates');
    },
  };
}
