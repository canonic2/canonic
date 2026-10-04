import path from 'node:path';
import { loadProject } from '../src/modules/configuration/index.ts';
const directory = path.resolve(process.argv[2] || process.cwd());
const config = await loadProject(directory);
console.log(
  JSON.stringify({ project: directory, configured: Boolean(config.runtime), config }, null, 2),
);
