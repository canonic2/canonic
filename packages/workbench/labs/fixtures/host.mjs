// Host conditions shared by lab runners: one lab run at a time, waiting for a
// quiet machine, and recording what else was running.
import { spawnSync } from 'node:child_process';
import fs from 'node:fs';
import os from 'node:os';
import path from 'node:path';

const LOCK = path.join(os.tmpdir(), 'canonic-workbench-labs', 'run.lock');

// Refuses to start while another lab run holds the lock; a lock whose
// process is gone is stale and replaced.
export function acquireLabLock(lab) {
  fs.mkdirSync(path.dirname(LOCK), { recursive: true });
  try {
    const held = JSON.parse(fs.readFileSync(LOCK, 'utf8'));
    let alive = false;
    try { process.kill(held.pid, 0); alive = true; } catch (_) {}
    if (alive && held.pid !== process.pid) throw new Error(`Another lab run is in progress (${held.lab}, pid ${held.pid}). Labs run one at a time.`);
  } catch (error) { if (error.code !== 'ENOENT' && !(error instanceof SyntaxError)) throw error; }
  fs.writeFileSync(LOCK, JSON.stringify({ lab, pid: process.pid, started: new Date().toISOString() }));
  const release = () => { try { if (JSON.parse(fs.readFileSync(LOCK, 'utf8')).pid === process.pid) fs.unlinkSync(LOCK); } catch (_) {} };
  process.on('exit', release);
  for (const signal of ['SIGINT', 'SIGTERM']) process.on(signal, () => { release(); process.exit(130); });
  return release;
}

// The processes using the most CPU right now, excluding this run's own tree.
export function busiestProcesses(limit = 5) {
  const out = spawnSync('ps', ['-A', '-o', 'pid=,ppid=,pcpu=,comm='], { encoding: 'utf8' }).stdout || '';
  const rows = out.trim().split('\n').map(line => {
    const [pid, ppid, cpu, ...command] = line.trim().split(/\s+/);
    return { pid: Number(pid), ppid: Number(ppid), cpu: Number(cpu), command: path.basename(command.join(' ')) };
  });
  const own = new Set([process.pid]);
  for (let changed = true; changed;) { changed = false; for (const row of rows) if (own.has(row.ppid) && !own.has(row.pid)) { own.add(row.pid); changed = true; } }
  return rows.filter(row => !own.has(row.pid) && row.cpu > 0).sort((a, b) => b.cpu - a.cpu).slice(0, limit).map(row => `${row.command} ${row.cpu}%`);
}

export function hostSnapshot() {
  return { at: new Date().toISOString(), loadAverage: os.loadavg().map(value => Math.round(value * 100) / 100), busiest: busiestProcesses() };
}

// Waits until the one-minute load average is below `threshold`, checking
// every five seconds, for at most `maxWaitMs`. Returns how long it waited and
// whether the machine became quiet.
export async function waitForQuiet(threshold, maxWaitMs = 10 * 60 * 1000, log = () => {}) {
  const started = Date.now();
  let reported = 0;
  while (os.loadavg()[0] >= threshold) {
    if (Date.now() - started > maxWaitMs) return { quiet: false, waitedMs: Date.now() - started, snapshot: hostSnapshot() };
    if (Date.now() - reported > 60000) { reported = Date.now(); log(`Waiting for load below ${threshold}: ${os.loadavg()[0].toFixed(2)} (${busiestProcesses(3).join(', ')})`); }
    await new Promise(resolve => setTimeout(resolve, 5000));
  }
  return { quiet: true, waitedMs: Date.now() - started };
}
