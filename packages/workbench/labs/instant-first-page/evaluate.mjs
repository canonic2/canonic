// The Rust index's fallback for definitions whose list fields are not
// literals: evaluates exactly the files it is given, in one esbuild build,
// with the shipping compiler (loaded read-only).
//
//   node labs/instant-first-page/evaluate.mjs <project>  < files (one per line)
//
// Prints one JSON object: { definitions: { <file>: { value, files } }, errors }.
// Files are absolute; `files` lists what each definition read, for the saved
// index to check.
import fs from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { Compiler } = require(path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'preview', 'compiler.cjs'));

const root = path.resolve(process.argv[2]);
const files = fs.readFileSync(0, 'utf8').split('\n').filter(Boolean);
const compiler = new Compiler(root, {});
const definitions = {};
const errors = {};
await compiler.load(files);
for (const file of files) {
  try { definitions[file] = await compiler.definition(file); }
  catch (error) { errors[file] = String(error.message || error); }
}
process.stdout.write(JSON.stringify({ definitions, errors }));
