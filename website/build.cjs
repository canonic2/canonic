const fs = require('node:fs');
const path = require('node:path');

const TARGETS = [
  'darwin-arm64', 'darwin-x64',
  'win32-x64', 'win32-arm64',
  'linux-x64', 'linux-arm64',
];
const START = '<!-- downloads:start -->';
const END = '<!-- downloads:end -->';

function versionParts(release) {
  const match = /^workbench\/v(\d+)\.(\d+)\.(\d+)$/.exec(release.tag_name || '');
  return match && match.slice(1).map(Number);
}

function completeAssets(release) {
  const names = new Set((release.assets || []).map(asset => asset.name));
  return TARGETS.every(target => names.has(`canonic-workbench-${target}.vsix`));
}

function selectRelease(releases) {
  return releases.filter(release =>
    !release.draft && !release.prerelease && versionParts(release) && completeAssets(release)
  ).sort((a, b) => {
    const av = versionParts(a), bv = versionParts(b);
    return bv[0] - av[0] || bv[1] - av[1] || bv[2] - av[2];
  })[0] || null;
}

function renderDownloads(html, release, repository) {
  const start = html.indexOf(START), end = html.indexOf(END);
  if (start < 0 || end < start || html.indexOf(START, start + START.length) >= 0) {
    throw new Error('Expected one downloads section in website/index.html');
  }
  let section = html.slice(start + START.length, end);
  if (!release) {
    section = '\n      <p class="lede">Workbench downloads will appear after the first successful workbench release.</p>\n      ';
  } else {
    const parts = versionParts(release);
    if (!parts || !completeAssets(release)) throw new Error('Workbench release is missing its version or platform files');
    const releasePrefix = `https://github.com/${repository}/releases`;
    if (!release.html_url.startsWith(`${releasePrefix}/tag/`)) throw new Error('Unexpected workbench release URL');
    for (const target of TARGETS) {
      const name = `canonic-workbench-${target}.vsix`;
      const oldUrl = `${releasePrefix}/latest/download/${name}`;
      const asset = release.assets.find(item => item.name === name);
      if (!asset.browser_download_url.startsWith(`${releasePrefix}/download/`)) {
        throw new Error(`Unexpected download URL for ${name}`);
      }
      if (!section.includes(oldUrl)) throw new Error(`Missing website link for ${name}`);
      section = section.replace(oldUrl, asset.browser_download_url);
    }
    section = section.replace(`${releasePrefix}/latest`, release.html_url);
    section = section.replace('latest workbench release on GitHub', `workbench v${parts.join('.')} on GitHub`);
  }
  return html.slice(0, start + START.length) + section + html.slice(end);
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
  const endpoint = `repos/${repository}/releases`;
  const releases = await githubJson(`${endpoint}?per_page=100`, token);
  if (tag) {
    const release = releases.find(item => item.tag_name === tag);
    if (!release) throw new Error(`Release ${tag} was not found`);
    if (!completeAssets(release)) throw new Error(`Release ${tag} is missing platform files`);
    return release;
  }
  return selectRelease(releases);
}

async function buildPublic(output, repository, tag, token) {
  if (!/^[\w.-]+\/[\w.-]+$/.test(repository)) throw new Error('Invalid GitHub repository name');
  const release = await resolveRelease(repository, tag, token);
  const root = __dirname;
  const html = renderDownloads(fs.readFileSync(path.join(root, 'index.html'), 'utf8'), release, repository);
  fs.mkdirSync(output, { recursive: true });
  fs.writeFileSync(path.join(output, 'index.html'), html);
  for (const name of ['style.css', 'canonic.svg']) fs.copyFileSync(path.join(root, name), path.join(output, name));
  fs.cpSync(path.join(root, 'images'), path.join(output, 'images'), { recursive: true });
  fs.writeFileSync(path.join(output, '.nojekyll'), '');
  console.log(release ? `Website links to ${release.tag_name}` : 'Website has no workbench release yet');
}

if (require.main === module) {
  const output = process.argv[2];
  if (!output) throw new Error('Usage: node website/build.cjs <output-directory>');
  buildPublic(path.resolve(output), process.env.GITHUB_REPOSITORY || 'canonic2/canonic',
    process.env.WORKBENCH_TAG, process.env.GITHUB_TOKEN
  ).catch(error => { console.error(error.message); process.exitCode = 1; });
}

module.exports = { renderDownloads, selectRelease };
