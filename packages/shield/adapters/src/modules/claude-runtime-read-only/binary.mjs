// Parent-owned evidence about the reviewed image. This is a receipt fence,
// not atomic image execution or confinement of another same-user process.
import { open, lstat } from 'node:fs/promises';
import { constants } from 'node:fs';
import { dirname, isAbsolute } from 'node:path';
import { createHash } from 'node:crypto';

const VERSION = ['dev', 'ino', 'mode', 'nlink', 'size', 'mtimeNs', 'ctimeNs'];
const equal = (a, b) => VERSION.every(key => a[key] === b[key]);
const failure = () => Object.assign(Error('Shield executable identity is unavailable or changed. Recreate the client with the verified binary and trusted digest.'), { code: 'binary_identity' });

/** Fixed native CLI launch locators; these are not operation environment evidence. */
export function launchEnvironment(source) {
  const environment = {};
  for (const name of ['HOME', 'USERPROFILE', 'XDG_CACHE_HOME', 'XDG_CONFIG_HOME', 'XDG_DATA_HOME', 'XDG_STATE_HOME',
    'APPDATA', 'LOCALAPPDATA', 'TMPDIR', 'TMP', 'TEMP', 'LANG', 'LC_ALL', 'LC_CTYPE', 'TZ', 'TERM', 'NO_COLOR']) {
    if (typeof source[name] === 'string' && !source[name].includes('\0')) environment[name] = source[name];
  }
  return Object.freeze(environment);
}
function strong(value, directory) {
  if (!(directory ? value.isDirectory() : value.isFile()) || value.isSymbolicLink()
    || VERSION.some(key => typeof value[key] !== 'bigint') || value.ino <= 0n
    || value.nlink <= 0n || value.ctimeNs <= 0n || (!directory && (value.mode & 0o111n) === 0n)) throw failure();
  return value;
}

async function checked(handle, path, baseline, directory) {
  try {
    const [held, named] = await Promise.all([handle.stat({ bigint: true }), lstat(path, { bigint: true })]);
    if (!equal(strong(held, directory), baseline) || !equal(strong(named, directory), baseline)) throw failure();
  } catch { throw failure(); }
}

/** The digest must come from reviewed host configuration, never this child. */
export async function pinBinary(path, expectedDigest) {
  if (!['darwin', 'linux'].includes(process.platform)) throw Object.assign(Error('The reviewed SDK executable receipt is unsupported on this platform.'), { code: 'unsupported_platform' });
  if (!isAbsolute(path)
    || !/^[a-f0-9]{64}$/.test(expectedDigest ?? '') || !Number.isInteger(constants.O_NOFOLLOW)
    || !Number.isInteger(constants.O_DIRECTORY)) throw failure();
  let file;
  try { file = await open(path, constants.O_RDONLY | constants.O_NOFOLLOW); } catch { throw failure(); }
  let closed = false;
  const check = async () => {
    try { if (closed) throw failure(); await checked(file, path, baseline, false); }
    catch { throw failure(); }
  };
  let baseline;
  try {
    baseline = strong(await file.stat({ bigint: true }), false);
    if (baseline.size < 4n || baseline.size > 128n * 1024n * 1024n) throw failure();
    const hash = createHash('sha256');
    const bytes = Buffer.alloc(64 * 1024);
    let header;
    for (let offset = 0; offset < Number(baseline.size);) {
      const { bytesRead } = await file.read(bytes, 0, Math.min(bytes.length, Number(baseline.size) - offset), offset);
      if (bytesRead === 0) throw failure();
      if (offset === 0) header = bytes.subarray(0, 4).toString('hex');
      hash.update(bytes.subarray(0, bytesRead));
      offset += bytesRead;
    }
    const headers = process.platform === 'darwin'
      ? ['cffaedfe', 'feedfacf', 'cafebabe', 'bebafeca', 'cafebabf', 'bfbafeca'] : ['7f454c46'];
    if (!headers.includes(header) || hash.digest('hex') !== expectedDigest) throw failure();
    await check();
  } catch { await file.close(); throw failure(); }
  return Object.freeze({ check, close: async () => { if (!closed) { closed = true; await file.close(); } },
    // Fresh ancestor baselines avoid making unrelated earlier directory changes
    // a lifetime failure. All handles remain open until the child frame is checked.
    guard: async operation => {
      const ancestors = [];
      try {
        await check();
        for (let pathPart = dirname(path);;) {
          if (ancestors.length >= 64) throw failure();
          const handle = await open(pathPart, constants.O_RDONLY | constants.O_DIRECTORY | constants.O_NOFOLLOW).catch(() => { throw failure(); });
          try {
            const stamp = strong(await handle.stat({ bigint: true }), true);
            ancestors.push({ handle, path: pathPart, stamp });
          } catch { await handle.close(); throw failure(); }
          const parent = dirname(pathPart);
          if (parent === pathPart) break;
          pathPart = parent;
        }
        const verify = async () => {
          await check();
          for (const entry of ancestors) await checked(entry.handle, entry.path, entry.stamp, true);
          await check();
        };
        await verify();
        const result = await operation();
        await verify();
        return result;
      } finally { await Promise.allSettled(ancestors.map(entry => entry.handle.close())); }
    } });
}
