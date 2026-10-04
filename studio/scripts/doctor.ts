import { errorMessage } from '../src/platform/errors.ts';
import { access } from 'node:fs/promises';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import { defaultRuntime } from '../src/platform/paths.ts';
const exec = promisify(execFile);
const report: Record<string, string | { unavailable: string }> = {
  node: process.version,
  platform: `${process.platform}/${process.arch}`,
};
const commands: [string, string, string[]][] = [
  ['git', 'git', ['--version']],
  ['docker', 'docker', ['version', '--format', '{{.Server.Version}}']],
  ['compose', 'docker', ['compose', 'version', '--short']],
];
for (const [name, command, args] of commands) {
  try {
    report[name] = (await exec(command, args, { timeout: 10000 })).stdout.trim();
  } catch (error) {
    report[name] = { unavailable: errorMessage(error) };
  }
}
try {
  await access(process.env.STUDIO_IDE_RUNTIME || defaultRuntime);
  report.ide = 'installed';
} catch {
  report.ide = 'Run pnpm run setup';
}
console.log(JSON.stringify(report, null, 2));
if (typeof report.git !== 'string' || report.ide !== 'installed') process.exitCode = 1;
