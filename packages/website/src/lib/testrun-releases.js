import { listReleases } from './releases.js';

const repository = 'canonic2/canonic';
const targets = [
  ['aarch64-apple-darwin', 'macOS · Apple silicon'],
  ['x86_64-apple-darwin', 'macOS · Intel'],
  ['aarch64-pc-windows-msvc', 'Windows · ARM64'],
  ['x86_64-pc-windows-msvc', 'Windows · x64'],
  ['aarch64-unknown-linux-gnu', 'Linux · ARM64'],
  ['x86_64-unknown-linux-gnu', 'Linux · x64'],
];

export function selectTestrunRelease(releases, tag) {
  const base = `https://github.com/${repository}/releases`;
  const candidates = [];
  for (const release of releases) {
    const match = /^testrun\/v((0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*))$/.exec(release.tag_name);
    if (!match || release.draft || release.prerelease || (tag && release.tag_name !== tag)) continue;
    const [, version, ...parts] = match;
    const assets = new Set((release.assets || []).filter(asset => asset.state === 'uploaded').map(asset => asset.name));
    const download = `${base}/download/testrun/v${version}`;
    const files = [];
    for (const [target, label] of targets) {
      const name = `testrun-${version}-${target}.${target.endsWith('-msvc') ? 'zip' : 'tar.gz'}`;
      const signature = target.endsWith('-linux-gnu') ? `${name}.sigstore.json` : null;
      if (!assets.has(name) || !assets.has(`${name}.sha256`) || (signature && !assets.has(signature))) break;
      files.push({ label, archive: `${download}/${name}`, checksum: `${download}/${name}.sha256`,
        signature: signature && `${download}/${signature}` });
    }
    if (files.length === targets.length) {
      candidates.push({ version, parts: parts.map(Number), url: `${base}/tag/testrun/v${version}`, files });
    }
  }
  candidates.sort(({ parts: a }, { parts: b }) => b[0] - a[0] || b[1] - a[1] || b[2] - a[2]);
  const selected = candidates[0] || null;
  if (tag && !selected) throw new Error(`Release ${tag} is missing or lacks complete platform assets`);
  return selected;
}

export async function testrunDownloads() {
  const tag = process.env.TESTRUN_TAG;
  const release = selectTestrunRelease(await listReleases(repository, process.env.GITHUB_TOKEN));
  if (tag && (!release || `testrun/v${release.version}` !== tag)) {
    throw new Error(`${tag} is not the newest complete Testrun release`);
  }
  return release;
}
