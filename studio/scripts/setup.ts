import { createHash } from 'node:crypto';
import { mkdir, readFile, rename, rm, writeFile } from 'node:fs/promises';
import { execFileSync } from 'node:child_process';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const root = fileURLToPath(new URL('../', import.meta.url));
const version = '4.140.0';
const builds: Record<string, [string, string]> = {
  'darwin-x64': ['macos-amd64', 'a5393b6eed4aa68b084e724c3c565f805abd996c609356043119f0323e40cf52'],
  'darwin-arm64': [
    'macos-arm64',
    '82c7144406ac31c373acfa786b6705c7c5463d895f728fde2cb94402b945b301',
  ],
  'linux-x64': ['linux-amd64', '864c5d01c808ade57e4d12c708717be7a187219fded60428f263b9e2da9f6b48'],
  'linux-arm64': [
    'linux-arm64',
    'ae4b07153f2037b06d24749bc8004221fcbf3ffe317038401be0452f541bf200',
  ],
};
const build = builds[`${process.platform}-${process.arch}`];
if (!build) throw new Error('Studio supports macOS and Linux on x64/arm64.');
const [target, digest] = build;
const runtime = path.join(root, '.runtime');
const destination = path.join(runtime, 'code-server');
try {
  const installed: unknown = JSON.parse(
    await readFile(path.join(destination, 'studio-runtime.json'), 'utf8'),
  );
  if (
    installed &&
    typeof installed === 'object' &&
    'version' in installed &&
    'target' in installed &&
    installed.version === version &&
    installed.target === target
  ) {
    console.log(`code-server ${version} already installed.`);
    process.exit(0);
  }
} catch {
  /* An absent or invalid installation marker requires setup. */
}
await mkdir(runtime, { recursive: true });
const archive = `code-server-${version}-${target}.tar.gz`;
console.log(`Downloading code-server ${version} (${target})…`);
const response = await fetch(
  `https://github.com/coder/code-server/releases/download/v${version}/${archive}`,
);
if (!response.ok) throw new Error(`Download failed: HTTP ${response.status}`);
const bytes = Buffer.from(await response.arrayBuffer());
if (createHash('sha256').update(bytes).digest('hex') !== digest)
  throw new Error('Runtime checksum does not match the pinned release.');
const archivePath = path.join(runtime, archive);
await writeFile(archivePath, bytes);
const staging = path.join(runtime, `install-${Date.now()}`);
await mkdir(staging);
try {
  execFileSync('tar', ['-xzf', archivePath, '-C', staging, '--strip-components=1']);
  await writeFile(
    path.join(staging, 'studio-runtime.json'),
    JSON.stringify({ version, target, digest }, null, 2),
  );
  await rm(destination, { recursive: true, force: true });
  await rename(staging, destination);
} finally {
  await rm(staging, { recursive: true, force: true });
  await rm(archivePath, { force: true });
}
console.log('Runtime ready. Run pnpm start.');
