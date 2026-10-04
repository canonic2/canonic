/* What every Workbench page shares: the repository, the section links under
   the product bar, and the release data, fetched once per build. */
import { base } from './base.js';
import { workbenchDownloads, workbenchReleases } from './releases.js';

export const repository = process.env.GITHUB_REPOSITORY || 'canonic2/canonic';
export const github = `https://github.com/${repository}`;
export const marketplace = 'https://marketplace.visualstudio.com/items?itemName=canonic.canonic-workbench';

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

export const download = { href: pages.install, label: 'Install' };

/* The smallest workbench.yaml, shown by the overview and the Install page. */
export const setupYaml = `name: Acme

collections:
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

/* Every published release, newest first. A build asks GitHub once and stops
   if it can't be reached. The dev server and Workbench previews ask on every
   render, and show a link to GitHub's releases when it can't be reached. */
let releasesRequest;
export function releases() {
  if (import.meta.env.DEV) {
    return workbenchReleases({ repository, token: process.env.GITHUB_TOKEN, cache: false }).catch(error => {
      console.warn(`Changelog unavailable in dev: ${error.message}`);
      return null;
    });
  }
  releasesRequest ||= workbenchReleases({ repository, token: process.env.GITHUB_TOKEN });
  return releasesRequest;
}
