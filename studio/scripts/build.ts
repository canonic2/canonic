import { cp, mkdir, readFile, writeFile, rm } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { packageRoot } from '../src/platform/paths.ts';

const destination = path.join(packageRoot, 'dist');
await rm(destination, { recursive: true, force: true });
execFileSync(process.execPath, [path.join(packageRoot, 'node_modules/typescript/bin/tsc')], {
  cwd: packageRoot,
  stdio: 'inherit',
});
await mkdir(path.join(destination, 'app'), { recursive: true });
for (const file of ['studio.css']) {
  await cp(path.join(packageRoot, 'app', file), path.join(destination, 'app', file));
}
await writeFile(
  path.join(destination, 'app/index.html'),
  (await readFile(path.join(packageRoot, 'app/index.html'), 'utf8')).replace(
    'src="studio.ts"',
    'src="studio.js"',
  ),
);
