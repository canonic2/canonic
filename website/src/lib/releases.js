/* The workbench release the Install section links to. A build asks GitHub for
   the newest complete release, or the one a workbench tag names; the dev
   server links to GitHub's latest release without asking. */

export const TARGETS = [
  { target: 'darwin-arm64', computer: 'Mac with Apple silicon (M1 and later)' },
  { target: 'darwin-x64', computer: 'Mac with Intel' },
  { target: 'win32-x64', computer: 'Windows' },
  { target: 'win32-arm64', computer: 'Windows on Arm' },
  { target: 'linux-x64', computer: 'Linux' },
  { target: 'linux-arm64', computer: 'Linux on Arm' },
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

export function selectRelease(releases) {
  return releases.filter(release =>
    !release.draft && !release.prerelease && versionParts(release) && completeAssets(release)
  ).sort((a, b) => {
    const av = versionParts(a), bv = versionParts(b);
    return bv[0] - av[0] || bv[1] - av[1] || bv[2] - av[2];
  })[0] || null;
}

export function latestDownloads(repository) {
  const releases = `https://github.com/${repository}/releases`;
  return {
    version: null,
    url: `${releases}/latest`,
    files: TARGETS.map(({ target, computer }) => ({
      target, computer, name: fileName(target), url: `${releases}/latest/download/${fileName(target)}`,
    })),
  };
}

export function releaseDownloads(release, repository) {
  const parts = versionParts(release);
  if (!parts || !completeAssets(release)) throw new Error('Workbench release is missing its version or platform files');
  const releases = `https://github.com/${repository}/releases`;
  if (!release.html_url.startsWith(`${releases}/tag/`)) throw new Error('Unexpected workbench release URL');
  return {
    version: parts.join('.'),
    url: release.html_url,
    files: TARGETS.map(({ target, computer }) => {
      const name = fileName(target);
      const asset = release.assets.find(item => item.name === name);
      if (!asset.browser_download_url.startsWith(`${releases}/download/`)) {
        throw new Error(`Unexpected download URL for ${name}`);
      }
      return { target, computer, name, url: asset.browser_download_url };
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

async function resolveRelease(repository, tag, token) {
  const releases = await githubJson(`repos/${repository}/releases?per_page=100`, token);
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
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error('Invalid GitHub repository name');
  if (latest) return latestDownloads(repository);
  const release = await resolveRelease(repository, tag, token);
  return release && releaseDownloads(release, repository);
}
