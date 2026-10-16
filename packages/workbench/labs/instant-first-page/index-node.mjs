// I1's shipping side: the shipping compiler's page index, loaded read-only,
// timed from this process starting.
//
//   node labs/instant-first-page/index-node.mjs <project> [--out <file>]
//
// Prints { ms, previews, docs, errors } with ms from process start; --out
// writes the index itself for comparison with the Rust index.
import fs from 'node:fs';
import { createRequire } from 'node:module';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);
const { Compiler } = require(path.join(path.dirname(fileURLToPath(import.meta.url)), '..', '..', 'preview', 'compiler.cjs'));

const root = path.resolve(process.argv[2]);
const outIndex = process.argv.indexOf('--out');
const index = await new Compiler(root, {}).index();
const ms = performance.now();
if (outIndex !== -1) fs.writeFileSync(process.argv[outIndex + 1], JSON.stringify(index));
process.stdout.write(JSON.stringify({ ms, previews: index.previews.length, docs: index.docs.length, errors: index.errors.length }) + '\n');
