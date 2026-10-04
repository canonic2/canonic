import { access, readdir, readFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { packageRoot } from '../src/platform/paths.ts';
import { validateProject } from '../src/modules/configuration/index.ts';
import ts from 'typescript';

async function check(directory: string) {
  for (const entry of await readdir(directory, { withFileTypes: true })) {
    const file = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      if (!['node_modules', '.pnpm-store', 'dist'].includes(entry.name)) await check(file);
      continue;
    }
    if (/\.(ts|cts|cjs|js)$/.test(file) && !file.endsWith('.d.ts')) {
      const source = await readFile(file, 'utf8');
      if (/\.(ts|cts)$/.test(file)) {
        const result = ts.transpileModule(source, {
          fileName: file,
          reportDiagnostics: true,
          compilerOptions: { target: ts.ScriptTarget.ES2023, module: ts.ModuleKind.NodeNext },
        });
        const errors =
          result.diagnostics?.filter((item) => item.category === ts.DiagnosticCategory.Error) || [];
        if (errors.length)
          throw new Error(
            `${file}: ${errors.map((item) => ts.flattenDiagnosticMessageText(item.messageText, '\n')).join('\n')}`,
          );
      } else execFileSync(process.execPath, ['--check', file]);
      if (
        directory.includes(`${path.sep}src`) &&
        /(?:from\s+|import\s*\()(['"])electron\1/.test(source)
      )
        throw new Error(`Domain imports Electron: ${file}`);
      if (
        !file.includes(`${path.sep}test${path.sep}`) &&
        /(?:from\s+|import\s*\(|require\s*\()(['"])[^'"]*(?:\/test\/|fixtures)[^'"]*\1/.test(source)
      )
        throw new Error(`Production imports a fixture: ${file}`);
    }
    if (file.endsWith('.json')) {
      const value: unknown = JSON.parse(await readFile(file, 'utf8'));
      if (entry.name === 'studio.config.json') validateProject(value);
    }
    if (file.endsWith('.md')) {
      const source = await readFile(file, 'utf8');
      for (const match of source.matchAll(/\[[^\]]*\]\(([^)]+)\)/g)) {
        const target = match[1]?.split('#')[0];
        if (!target || /^(?:[a-z]+:|\/)/i.test(target)) continue;
        await access(path.resolve(directory, decodeURIComponent(target)));
      }
      for (const match of source.matchAll(/```json\s*\n([\s\S]*?)```/g)) {
        const example: unknown = JSON.parse(match[1]);
        if (example && typeof example === 'object' && 'version' in example)
          validateProject(example);
      }
    }
  }
}
for (const directory of ['src', 'app', 'scripts', 'config', 'test', 'docs', 'specs'])
  await check(path.join(packageRoot, directory));
await access(path.join(packageRoot, 'pnpm-lock.yaml'));
const manifest = JSON.parse(await readFile(path.join(packageRoot, 'package.json'), 'utf8')) as {
  engines: { node: string };
  packageManager: string;
  dependencies: Record<string, string>;
  devDependencies: Record<string, string>;
};
if (manifest.engines.node !== '>=24.0.0' || !/^pnpm@\d+\.\d+\.\d+$/.test(manifest.packageManager))
  throw new Error('Studio requires Node 24+ and a pinned pnpm version.');
for (const [name, version] of Object.entries({
  ...manifest.dependencies,
  ...manifest.devDependencies,
}))
  if (!/^\d+\.\d+\.\d+(?:-[\w.-]+)?$/.test(version))
    throw new Error(`Dependency must use an exact version: ${name}`);
console.log('Source syntax, JSON, recipes, documentation links and toolchain contracts are valid.');
