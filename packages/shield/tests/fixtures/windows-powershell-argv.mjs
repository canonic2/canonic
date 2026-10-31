// Native Windows PowerShell 5.1 argv marshalling, under the fixture's owned Job.
import assert from 'node:assert/strict';
import { writeFile } from 'node:fs/promises';
import { join } from 'node:path';
import { execFile } from 'node:child_process';
import { promisify } from 'node:util';

const exec = promisify(execFile);
const [root, shell] = process.argv.slice(2);
assert.equal(process.platform, 'win32');
const recorder = join(root, 'record-argv.mjs');
await writeFile(recorder, `process.stdout.write(JSON.stringify(process.argv.slice(2)).replace(/[^\\x00-\\x7f]/g, c => '\\\\u' + c.charCodeAt(0).toString(16).padStart(4, '0')));`);
const literal = text => `'${text.replaceAll("'", "''")}'`;
for (const [source, expected] of [
  ['status --short', ['status', '--short']],
  [String.raw`-C 'C:\Acme Tools\project' status`, ['-C', String.raw`C:\Acme Tools\project`, 'status']],
  ["'C:\\acme\\project\\'", ["C:\\acme\\project\\"]],
  [`'don''t' "notes here.txt"`, ["don't", 'notes here.txt']],
  [`'$HOME; $(whoami)' 'literal\`text'`, ['$HOME; $(whoami)', 'literal`text']],
  [`-- -x --flag=value 'café.txt'`, ['--', '-x', '--flag=value', 'café.txt']],
]) {
  const script = `& ${literal(process.execPath)} ${literal(recorder)} ${source}; if ($LASTEXITCODE -ne 0) { exit 2 }`;
  const running = exec(shell, ['-NoLogo', '-NoProfile', '-NonInteractive', '-EncodedCommand', Buffer.from(script, 'utf16le').toString('base64')], { cwd: root, timeout: 10_000, maxBuffer: 65_536 });
  running.child.stdin.end();
  const { stdout } = await running;
  assert.deepEqual(JSON.parse(stdout), expected, source);
}
await writeFile(join(root, 'powershell-argv-result'), 'verified');
