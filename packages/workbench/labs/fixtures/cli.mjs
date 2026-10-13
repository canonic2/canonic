// node labs/fixtures/cli.mjs <empty-directory> [--size small|large|formats] [--seed N] [--install]
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { generate } from './acme-app.mjs';
import { generate as generateFormats } from './formats.mjs';
import { install } from './install.mjs';

const args = process.argv.slice(2);
const option = (name, fallback) => { const index = args.indexOf('--' + name); return index === -1 ? fallback : args[index + 1]; };
const target = args.find((arg, index) => !arg.startsWith('--') && !['--size', '--seed'].includes(args[index - 1]));
if (!target) { console.error('Usage: node labs/fixtures/cli.mjs <empty-directory> [--size small|large|formats] [--seed N] [--install]'); process.exit(2); }
const size = option('size', 'large');
const manifest = (size === 'formats' ? generateFormats : generate)(path.resolve(target), { size, seed: Number(option('seed', 1)) });
console.log(`Generated ${manifest.size} fixture: ${manifest.previews.length} previews, ${manifest.files} files, ${(manifest.sourceBytes / 1e6).toFixed(2)} MB of source`);
if (args.includes('--install')) {
  const here = path.dirname(fileURLToPath(import.meta.url));
  install(path.resolve(target), path.join(here, 'locks', size + '.package-lock.json'));
}
