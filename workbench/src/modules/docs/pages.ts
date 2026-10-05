/* The docs a space has: every page workbench.yaml gives Markdown, a .md src
   or `docs`, with its docs lenses, then those defineDocs definitions
   declare, unless the YAML already lists their Markdown file. One entry per
   Markdown file; the first page to claim a file has it. Shared by the server
   and the portable export. */

import type { DocsLens, DocsPageEntry } from './docs-service.ts';

interface PageItem {
  src: string;
  label: string;
  markdown?: string;
  lens?: string;
  implementations?: Record<string, { examples?: string }>;
}

interface Collection {
  items?: (PageItem | { group: string; items?: PageItem[] })[];
}

interface Implementation {
  label: string;
  kind: string;
  adapter?: string;
  styles?: string[];
  environment?: string;
}

export interface DocsConfig {
  collections?: Collection[];
  implementations?: Record<string, Implementation>;
}

function items(collections: readonly Collection[] | undefined): PageItem[] {
  const out: PageItem[] = [];
  for (const collection of collections ?? []) {
    for (const entry of collection.items ?? []) {
      if ('group' in entry) out.push(...(entry.items ?? []));
      else if (entry && entry.src) out.push(entry);
    }
  }
  return out;
}

export function docsPages(config: DocsConfig | null | undefined, discovered: readonly DocsPageEntry[] = []): DocsPageEntry[] {
  const out: DocsPageEntry[] = [];
  const srcs = new Set<string>();
  for (const item of items(config?.collections)) {
    if (!item.markdown || srcs.has(item.markdown)) continue;
    srcs.add(item.markdown);
    out.push({
      src: item.markdown,
      page: item.src,
      label: item.label,
      lens: item.lens ?? null,
      lenses: Object.keys(item.implementations ?? {}).flatMap((key): DocsLens[] => {
        const impl = config?.implementations?.[key];
        const examples = item.implementations?.[key]?.examples;
        if (!impl || impl.kind !== 'docs' || !impl.adapter || !examples) return [];
        return [{ key, label: impl.label, adapter: impl.adapter, styles: impl.styles ?? [],
          ...(impl.environment ? { environment: impl.environment } : {}), examples }];
      }),
    });
  }
  return out.concat(discovered.filter(entry => !srcs.has(entry.src)));
}
