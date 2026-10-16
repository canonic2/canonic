// The Rust index's and host's fallback for what Rust cannot read as
// literals: evaluates exactly the files it is given, in one esbuild build,
// with the shipping compiler (loaded read-only).
//
//   node labs/instant-first-page/evaluate.mjs <project>  < files (one per line)
//   node labs/instant-first-page/evaluate.mjs <project> --config
//
// Prints one JSON object: { definitions: { <file>: { value, files } }, errors },
// with absolute files and what each definition read, for the saved index to
// check; or, with --config, { config: { environment, adapters }, files } with
// the config fields entry generation uses and the files the config read.
import fs from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { Compiler } = require(path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'server/node/preview', 'compiler.cjs'));

const root = path.resolve(process.argv[2]);
const compiler = new Compiler(root, {});
if (process.argv.includes('--config')) {
  const config = await compiler.settings();
  const adapters = {};
  for (const [name, adapter] of Object.entries(config.adapters || {})) if (adapter) adapters[name] = { runtime: adapter.runtime || null };
  process.stdout.write(JSON.stringify({ config: { environment: config.environment ?? null, adapters }, files: [...compiler.configFiles.keys()] }));
} else {
  const files = fs.readFileSync(0, 'utf8').split('\n').filter(Boolean);
  const definitions = {};
  const errors = {};
  await compiler.load(files);
  for (const file of files) {
    try { definitions[file] = await compiler.definition(file); }
    catch (error) { errors[file] = String(error.message || error); }
  }
  process.stdout.write(JSON.stringify({ definitions, errors }));
}
