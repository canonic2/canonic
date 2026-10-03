import path from 'node:path';
import { fileURLToPath } from 'node:url';

const docsRoot = path.resolve(fileURLToPath(new URL('../../../workbench/docs/', import.meta.url)));

export function docsHref(id, base = '/') {
  return `${base.replace(/\/?$/, '/')}workbench/docs/${id === 'index' ? '' : `${id.replace(/\/index$/, '')}/`}`;
}

export function docTitle(entry) {
  return entry.body.match(/^#\s+(.+)$/m)?.[1].replace(/`/g, '') || entry.id;
}

export const docGroups = [
  { title: 'Getting started', ids: ['index', 'getting-started', 'pages-and-states', 'canvas', 'markup-and-handoff'] },
  { title: 'Connect your implementation', ids: ['workbench-previews', 'lenses', 'storybook', 'ios-simulator', 'windows'] },
  { title: 'Reference', ids: ['configuration', 'design-system-export', 'extension', 'troubleshooting'] },
];

export function docsNavigation(entries) {
  const byId = new Map(entries.map(entry => [entry.id, entry]));
  const groups = docGroups.map(group => ({
    title: group.title,
    entries: group.ids.map(id => byId.get(id)).filter(Boolean),
  }));
  const listed = new Set(docGroups.flatMap(group => group.ids));
  const rest = entries.filter(entry => !listed.has(entry.id)).sort((a, b) =>
    Number(b.id.endsWith('/index')) - Number(a.id.endsWith('/index')) || docTitle(a).localeCompare(docTitle(b)));
  if (rest.length) groups.push({ title: 'More guides', entries: rest });
  return groups;
}

// Keep repository Markdown links usable on the website, including fragments.
// Links outside the published docs lead to the corresponding repository file.
export function rewriteDocLink(url, source, base = '/') {
  if (!source || /^(?:[a-z][a-z\d+.-]*:|\/|#)/i.test(url)) return url;
  const match = /^([^?#]+)(.*)$/.exec(url);
  if (!match) return url;
  const resolved = path.resolve(path.dirname(source), decodeURIComponent(match[1]));
  const relative = path.relative(docsRoot, resolved).split(path.sep).join('/');
  if (!relative.startsWith('../')) {
    if (!/\.md$/i.test(match[1])) return url;
    const id = relative.replace(/\.md$/i, '').replace(/(^|\/)README$/, '$1index');
    return docsHref(id, base) + match[2];
  }
  const workbenchRoot = path.dirname(docsRoot);
  const repositoryPath = path.relative(workbenchRoot, resolved).split(path.sep).join('/');
  if (repositoryPath.startsWith('../')) return url;
  return `https://github.com/canonic2/canonic/blob/main/workbench/${repositoryPath}${match[2]}`;
}

// Links that leave the site open in a new tab.
export function isExternalLink(href, site) {
  if (!/^https?:\/\//i.test(href || '')) return false;
  return !site || new URL(href).origin !== new URL(site).origin;
}

export function docsMarkdown() {
  return {
    name: 'workbench-docs-links',
    hooks: {
      'astro:config:setup': ({ config }) => {
        config.markdown.processor.options.hastPlugins.push({
          name: 'workbench-docs-external-links',
          element: {
            filter: ['a'],
            visit: (node, context) => {
              if (!isExternalLink(node.properties?.href, config.site)) return;
              context.setProperty(node, 'target', '_blank');
              context.setProperty(node, 'rel', 'noopener');
            },
          },
        });
        // Extend Astro's default Sätteri processor without adding a renderer.
        const rewrite = (node, context) => {
          const source = context.fileURL && fileURLToPath(context.fileURL);
          if (!source || !source.startsWith(docsRoot + path.sep)) return;
          context.setProperty(node, 'url', rewriteDocLink(node.url, source, config.base));
        };
        config.markdown.processor.options.mdastPlugins.push({
          name: 'workbench-docs-links',
          link: rewrite,
          definition: rewrite,
        });
      },
    },
  };
}
