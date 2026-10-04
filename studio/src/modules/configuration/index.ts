import { errorMessage, errorCode } from '../../platform/errors.ts';
import { Ajv } from 'ajv';
import type { Schema } from 'ajv';
import { readFile } from 'node:fs/promises';
import path from 'node:path';
import { packageRoot } from '../../platform/paths.ts';
import type { ProjectConfig } from './types.ts';

const schema = JSON.parse(
  await readFile(path.join(packageRoot, 'config/project.schema.json'), 'utf8'),
) as Schema;
const ajv = new Ajv({ allErrors: true, useDefaults: true });
const validate = ajv.compile<ProjectConfig>(schema);

export function validateProject(input: unknown): ProjectConfig {
  const config = structuredClone(input);
  if (!validate(config))
    throw new Error(
      `Invalid studio.config.json: ${ajv.errorsText(validate.errors, { separator: '; ' })}`,
    );
  if (config.runtime) {
    const runtime = config.runtime;
    runtime.ports ||= ['PORT'];
    runtime.previewPort ||= runtime.ports[0];
    if (!runtime.ports.includes(runtime.previewPort))
      throw new Error('previewPort must name an allocated port.');
    for (const key of Object.keys(runtime.env || {})) {
      if (
        key.startsWith('STUDIO_') ||
        key.startsWith('XDG_') ||
        key === 'ELECTRON_RUN_AS_NODE' ||
        runtime.ports.includes(key)
      )
        throw new Error(`Reserved runtime environment variable: ${key}`);
    }
    if (
      runtime.compose &&
      (path.isAbsolute(runtime.compose.file) || runtime.compose.file.split(/[\\/]/).includes('..'))
    )
      throw new Error('Compose file must be inside the checkout.');
  }
  return config;
}

export async function loadProject(checkout: string): Promise<ProjectConfig> {
  const file = path.join(checkout, 'studio.config.json');
  try {
    return validateProject(JSON.parse(await readFile(file, 'utf8')));
  } catch (error) {
    if (errorCode(error) === 'ENOENT') return { version: 1 };
    throw new Error(`${file}: ${errorMessage(error)}`, { cause: error });
  }
}
