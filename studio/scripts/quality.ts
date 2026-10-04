import { spawn } from 'node:child_process';
import { packageRoot } from '../src/platform/paths.ts';

const full = process.argv.includes('--full');
const checks = [
  'check:lockfile',
  'fixture:install',
  'format:check',
  'typecheck',
  'typecheck:fixture',
  'lint',
  'analyze',
  'circular',
  'check:contracts',
  'test:coverage',
  'build',
  'check:build',
];
if (full)
  checks.push('smoke:startup', 'smoke', 'test:fullstack', 'smoke:fullstack', 'check:security');

for (const name of checks) {
  console.log(`\nStudio check: ${name}`);
  const code = await new Promise<number>((resolve, reject) => {
    const child = spawn('pnpm', ['run', name], { cwd: packageRoot, stdio: 'inherit' });
    child.once('error', reject);
    child.once('exit', (code) => resolve(code ?? 1));
  });
  if (code) process.exit(code);
}
console.log(`\nStudio ${full ? 'full ' : ''}checks passed.`);
