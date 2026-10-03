/* What every Workbench page shares: the repository, the section links under
   the product bar, and the release data, fetched once per build. */
import { base } from './base.js';
import { workbenchDownloads, workbenchReleases } from './releases.js';

export const repository = process.env.GITHUB_REPOSITORY || 'canonic2/canonic';
export const github = `https://github.com/${repository}`;

export const pages = {
  overview: base,
  docs: `${base}workbench/docs/`,
  install: `${base}workbench/install/`,
  changelog: `${base}workbench/changelog/`,
};

/* The section links. `current` is the label of the page being shown. */
export function workbenchNav(current) {
  return [
    { href: pages.overview, label: 'Overview' },
    { href: pages.docs, label: 'Docs' },
    { href: pages.install, label: 'Install' },
    { href: pages.changelog, label: 'Changelog' },
    { href: github, label: 'GitHub', external: true },
  ].map(item => ({ ...item, current: item.label === current }));
}

export const download = { href: pages.install, label: 'Download' };

/* The smallest workbench.yaml, shown by the overview and the Install page. */
export const setupYaml = `name: Acme

sections:
  - name: Pages
    icon: file-text
    items:
      - label: Sign in
        src: pages/sign-in.html`;

/* The release the Install sections link to: the newest complete one, or the
   one WORKBENCH_TAG names. Null before the first release. See releases.js. */
let downloadsRequest;
export function downloads() {
  downloadsRequest ||= workbenchDownloads({
    repository,
    tag: process.env.WORKBENCH_TAG,
    token: process.env.GITHUB_TOKEN,
    latest: import.meta.env.DEV,
  }).then(result => {
    if (!import.meta.env.DEV) {
      console.log(result ? `Website links to workbench v${result.version}` : 'Website has no workbench release yet');
    }
    return result;
  });
  return downloadsRequest;
}

/* Every complete release, newest first. A build stops if GitHub can't be
   reached; the dev server shows a link to GitHub's releases instead. */
let releasesRequest;
export function releases() {
  releasesRequest ||= workbenchReleases({ repository, token: process.env.GITHUB_TOKEN }).catch(error => {
    if (!import.meta.env.DEV) throw error;
    console.warn(`Changelog unavailable in dev: ${error.message}`);
    releasesRequest = undefined;
    return null;
  });
  return releasesRequest;
}
