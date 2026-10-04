import { access, readFile, readdir } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import assert from 'node:assert/strict';
import { packageRoot } from '../src/platform/paths.ts';

const output = path.join(packageRoot, 'dist');
for (const file of [
  'LICENSE',
  'app/main.js',
  'app/preload.cjs',
  'app/studio.js',
  'app/studio.css',
  'app/index.html',
  'src/application/studio.js',
])
  await access(path.join(output, file));
assert.equal(
  await readFile(path.join(output, 'LICENSE'), 'utf8'),
  await readFile(path.join(packageRoot, 'LICENSE'), 'utf8'),
);
const html = await readFile(path.join(output, 'app/index.html'), 'utf8');
assert.match(html, /src="studio\.js"/);
assert.doesNotMatch(html, /src="[^"]*\.ts"/);
const preload = await readFile(path.join(output, 'app/preload.cjs'), 'utf8');
assert.match(preload, /require\(['"]electron['"]\)/);
assert.doesNotMatch(preload, /require\(['"](?!electron['"])/);

async function check(directory: string): Promise<void> {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      await check(file);
      continue;
    }
    if (!/\.(js|cjs)$/.test(file)) continue;
    execFileSync(process.execPath, ['--check', file]);
    const source = await readFile(file, 'utf8');
    assert.doesNotMatch(
      source,
      /(?:from\s+|import\s*\()(['"])[^'"\n]*\.(?:ts|cts)\1/,
      `Source import in ${file}`,
    );
    if (
      file.startsWith(path.join(output, 'src') + path.sep) ||
      file.startsWith(path.join(output, 'app') + path.sep)
    ) {
      assert.doesNotMatch(
        source,
        /(?:from\s+|import\s*\()(['"])[^'"\n]*(?:\/test\/|fixtures)[^'"\n]*\1/,
        `Production fixture import in ${file}`,
      );
    }
  }
}
await check(output);
console.log('Desktop assets, sandbox preload, compiled syntax, and production imports are valid.');
