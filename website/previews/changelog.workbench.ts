import { definePreview } from '@canonic2/workbench';

// The build lists releases from GitHub's API; here the request is answered
// with fixed releases, so each state shows one of the page's cases.
const repo = 'https://github.com/canonic2/canonic';
const targets = ['darwin-arm64', 'darwin-x64', 'win32-x64', 'win32-arm64', 'linux-x64', 'linux-arm64'];
const release = (version: string, published: string, notes: string[], builds = true) => ({
  tag_name: `workbench/v${version}`,
  html_url: `${repo}/releases/tag/workbench/v${version}`,
  draft: false,
  prerelease: false,
  published_at: published,
  body: notes.map(note => `- ${note}`).join('\n'),
  assets: builds ? targets.map(target => ({
    name: `canonic-workbench-${target}.vsix`,
    browser_download_url: `${repo}/releases/download/workbench/v${version}/canonic-workbench-${target}.vsix`,
  })) : [],
});

const releases = [
  release('1.2.0', '2026-10-02T12:00:00Z', ['Add a Reset button to preview controls.', 'Fix the zoom menu in narrow windows.']),
  release('1.1.0', '2026-09-20T12:00:00Z', ['Add the Configure pages form.'], false),
  release('1.0.0', '2026-09-01T12:00:00Z', ['First release.'], false),
];

export default definePreview({
  id: 'website/changelog',
  title: 'Pages/Changelog',
  adapter: 'astro',
  source: { entry: '../src/pages/workbench/changelog.astro' },
  requests: { 'GET /repos/canonic2/canonic/releases': { body: releases } },
  states: {
    default: { label: 'Releases' },
    empty: { label: 'No releases yet', requests: { 'GET /repos/canonic2/canonic/releases': { body: [] } } },
    unavailable: { label: 'GitHub unavailable', requests: { 'GET /repos/canonic2/canonic/releases': { status: 503 } } },
  },
  links: {
    '/': 'website/overview',
    '/workbench/install/': 'website/install',
    '/workbench/changelog/': 'website/changelog',
    '/workbench/docs/': 'website/docs',
  },
  viewports: ['desktop', 'mobile', 'responsive'],
});
