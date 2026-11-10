// Local process lifecycle cases only. Never runs a native agent or provider.
import { spawn } from 'node:child_process';
import { writeFileSync, existsSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { command } from './native-command.mjs';

const file = fileURLToPath(import.meta.url);
const mode = process.argv[2];
if (mode === 'parent-disconnect') {
  void command(process.execPath, [file, 'wait'], { env: { FIXTURE_PID_FILE: process.env.FIXTURE_PID_FILE } });
  const ready = setInterval(() => {
    if (existsSync(process.env.FIXTURE_PID_FILE)) { clearInterval(ready); process.exit(0); }
  }, 10);
} else if (mode === 'exit') {
  process.stdout.write('Acme fixture complete.\n');
} else if (mode === 'leader-descendant') {
  const child = spawn(process.execPath, [file, 'wait'], { env: {}, stdio: ['ignore', 'inherit', 'inherit', 'ipc'] });
  child.on('message', () => {
    process.stdout.write(`${JSON.stringify({ leader: process.pid, descendant: child.pid })}\n`, () => process.exit(0));
  });
} else {
  process.on('SIGTERM', () => {});
  if (process.env.FIXTURE_PID_FILE) writeFileSync(process.env.FIXTURE_PID_FILE, JSON.stringify({ pid: process.pid, controller: process.ppid }));
  process.send?.('ready');
  if (mode === 'overflow') process.stdout.write(Buffer.alloc(3 * 1024 * 1024, 65));
  setInterval(() => {}, 1000);
}
