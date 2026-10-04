import { execFile } from 'node:child_process';
import { promisify } from 'node:util';
import path from 'node:path';
import { mkdir, realpath, writeFile } from 'node:fs/promises';
import { validateCompose } from './compose-policy.ts';
import type { RuntimeTask, ComposeModel } from './types.ts';
import type { RuntimeRecipe } from '../configuration/types.ts';
const exec = promisify(execFile);

export async function compose(task: RuntimeTask, args: string[]) {
  if (!task.composeFile) throw new Error('Compose launch plan is missing.');
  const result = await exec(
    'docker',
    [
      'compose',
      '--project-name',
      task.composeName,
      '--project-directory',
      task.checkout,
      '--file',
      task.composeFile,
      ...args,
    ],
    { cwd: task.checkout, env: task.env, timeout: 180000, maxBuffer: 4 * 1024 * 1024 },
  );
  if (!['config', 'ps'].includes(args[0]))
    task.composeOutput = (task.composeOutput + result.stdout + result.stderr).slice(-32000);
  return result.stdout;
}

export async function prepareCompose(task: RuntimeTask, recipe: RuntimeRecipe) {
  if (!recipe.compose) return;
  const { checkout, env } = task;
  task.composeFile = path.resolve(checkout, recipe.compose.file);
  if (!task.composeFile.startsWith(checkout + path.sep))
    throw new Error('Compose file must be inside the checkout.');
  task.composeFile = await realpath(task.composeFile);
  if (!task.composeFile.startsWith(checkout + path.sep))
    throw new Error('Compose file must be inside the checkout.');
  const config = JSON.parse(await compose(task, ['config', '--format', 'json'])) as ComposeModel;
  validateCompose(config, task);
  // Keep the resolved launch plan: editing source YAML must not repoint cleanup.
  const directory = path.join(env.XDG_CONFIG_HOME!, 'runtime');
  await mkdir(directory, { recursive: true });
  task.composeFile = path.join(directory, 'compose.json');
  // Compose interpolates again when reading this file; preserve resolved dollars.
  const literal = (value: unknown): unknown =>
    typeof value === 'string'
      ? value.replaceAll('$', '$$')
      : Array.isArray(value)
        ? value.map(literal)
        : value && typeof value === 'object'
          ? Object.fromEntries(Object.entries(value).map(([key, item]) => [key, literal(item)]))
          : value;
  await writeFile(task.composeFile, JSON.stringify(literal(config), null, 2), { mode: 0o600 });
  // Remember ownership before up: a partially failed up still needs down.
  task.composeStarted = true;
  await compose(task, [
    'up',
    '--detach',
    '--wait',
    '--wait-timeout',
    String(recipe.compose.waitTimeoutSeconds),
  ]);
}
