import { readFile, writeFile, rename } from 'node:fs/promises';
const [path, kind] = process.argv.slice(2);
if (kind === 'bytes') {
  const bytes = await readFile(path);
  const changed = Buffer.from(bytes);
  changed[changed.length - 1] ^= 1;
  await writeFile(path, changed);
  await writeFile(path, bytes);
} else if (kind === 'path' || kind === 'ancestor') {
  await rename(path, `${path}.away`);
  await rename(`${path}.away`, path);
} else throw Error('Unknown owned mutation fixture');
