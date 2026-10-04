/* Workbench releases for the Install sections and the changelog. A build asks
   GitHub for the releases: the Install sections link the newest complete one
   (all six files) or the one a workbench tag names, and the changelog lists
   every published one with whichever files it has. The dev server links the
   Install sections to GitHub's latest release without asking. */

export const TARGETS = [
  { target: 'darwin-arm64', computer: 'Mac with Apple silicon (M1 and later)', short: 'Mac, Apple silicon' },
  { target: 'darwin-x64', computer: 'Mac with Intel', short: 'Mac, Intel' },
  { target: 'win32-x64', computer: 'Windows', short: 'Windows' },
  { target: 'win32-arm64', computer: 'Windows on Arm', short: 'Windows on Arm' },
  { target: 'linux-x64', computer: 'Linux', short: 'Linux' },
  { target: 'linux-arm64', computer: 'Linux on Arm', short: 'Linux on Arm' },
];

function fileName(target) {
  return `canonic-workbench-${target}.vsix`;
}

function versionParts(release) {
  const match = /^workbench\/v(\d+)\.(\d+)\.(\d+)$/.exec(release.tag_name || '');
  return match && match.slice(1).map(Number);
}

function completeAssets(release) {
  const names = new Set((release.assets || []).map(asset => asset.name));
  return TARGETS.every(({ target }) => names.has(fileName(target)));
}

/* Published workbench releases, with or without files, newest version first. */
export function publishedReleases(releases) {
  return releases.filter(release => !release.draft && !release.prerelease && versionParts(release)).sort((a, b) => {
    const av = versionParts(a), bv = versionParts(b);
    return bv[0] - av[0] || bv[1] - av[1] || bv[2] - av[2];
  });
}

/* Published releases with all six platform files, newest version first. */
export function completeReleases(releases) {
  return publishedReleases(releases).filter(completeAssets);
}

export function selectRelease(releases) {
  return completeReleases(releases)[0] || null;
}

/* A release body as plain text blocks: paragraphs, and runs of list items.
   GitHub's generated notes end with a "Full Changelog" link, returned as
   `changes`; headings, comments, and the "New Contributors" list are left
   out, and Markdown links become their text. */
export function releaseNotes(body) {
  const blocks = [];
  let changes = null;
  let skipping = false;
  for (const raw of (body || '').split(/\r?\n/)) {
    const line = raw.trim();
    const full = /^\*\*Full Changelog\*\*:\s*(\S+)/.exec(line);
    if (full) { changes = full[1]; continue; }
    if (line.startsWith('#')) { skipping = /new contributors/i.test(line); continue; }
    if (!line || skipping || line.startsWith('<!--')) continue;
    const item = /^[-*+]\s+/.test(line);
    const text = line.replace(/^[-*+]\s+/, '').replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1').replace(/\*\*|__|`/g, '');
    if (!text) continue;
    const last = blocks[blocks.length - 1];
    if (item && last && last.items) last.items.push(text);
    else blocks.push(item ? { items: [text] } : { text });
  }
  return { blocks, changes };
}

export function latestDownloads(repository) {
  const releases = `https://github.com/${repository}/releases`;
  return {
    version: null,
    url: `${releases}/latest`,
    files: TARGETS.map(({ target, computer, short }) => ({
      target, computer, short, name: fileName(target), url: `${releases}/latest/download/${fileName(target)}`,
    })),
  };
}

export function releaseDownloads(release, repository) {
  if (!completeAssets(release)) throw new Error('Workbench release is missing its platform files');
  return releaseDetails(release, repository);
}

/* A release's version, notes, and links, with whichever platform files it has. */
export function releaseDetails(release, repository) {
  const parts = versionParts(release);
  if (!parts) throw new Error('Workbench release is missing its version');
  const repo = `https://github.com/${repository}`;
  const releases = `${repo}/releases`;
  if (!release.html_url.startsWith(`${releases}/tag/`)) throw new Error('Unexpected workbench release URL');
  const { blocks, changes } = releaseNotes(release.body);
  return {
    version: parts.join('.'),
    tag: release.tag_name,
    url: release.html_url,
    published: release.published_at || null,
    notes: blocks,
    // The generated "Full Changelog" link, kept only when it stays on this
    // repository; otherwise the tag's commit history.
    changes: changes && changes.startsWith(`${repo}/`) ? changes : `${repo}/commits/${release.tag_name}`,
    files: TARGETS.flatMap(({ target, computer, short }) => {
      const name = fileName(target);
      const asset = (release.assets || []).find(item => item.name === name);
      if (!asset) return [];
      if (!asset.browser_download_url.startsWith(`${releases}/download/`)) {
        throw new Error(`Unexpected download URL for ${name}`);
      }
      return [{ target, computer, short, name, url: asset.browser_download_url }];
    }),
  };
}

async function githubJson(endpoint, token) {
  const response = await fetch(`https://api.github.com/${endpoint}`, {
    headers: {
      Accept: 'application/vnd.github+json',
      'User-Agent': 'canonic-website-build',
      ...(token ? { Authorization: `Bearer ${token}` } : {}),
    },
  });
  if (!response.ok) throw new Error(`GitHub API returned ${response.status} for ${endpoint}`);
  return response.json();
}

// One request per repository for the whole build, shared by every page.
const releaseLists = new Map();
function checkRepository(repository) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error('Invalid GitHub repository name');
}

function listReleases(repository, token) {
  checkRepository(repository);
  if (!releaseLists.has(repository)) {
    releaseLists.set(repository, githubJson(`repos/${repository}/releases?per_page=100`, token).catch(error => {
      releaseLists.delete(repository);
      throw error;
    }));
  }
  return releaseLists.get(repository);
}

async function resolveRelease(repository, tag, token) {
  const releases = await listReleases(repository, token);
  if (tag) {
    const release = releases.find(item => item.tag_name === tag);
    if (!release) throw new Error(`Release ${tag} was not found`);
    if (!completeAssets(release)) throw new Error(`Release ${tag} is missing platform files`);
    return release;
  }
  return selectRelease(releases);
}

/* Null when no workbench release exists yet. */
export async function workbenchDownloads({ repository, tag, token, latest }) {
  checkRepository(repository);
  if (latest) return latestDownloads(repository);
  const release = await resolveRelease(repository, tag, token);
  return release && releaseDownloads(release, repository);
}

/* Every published workbench release, newest first, for the changelog. */
export async function workbenchReleases({ repository, token }) {
  return publishedReleases(await listReleases(repository, token)).map(release => releaseDetails(release, repository));
}
