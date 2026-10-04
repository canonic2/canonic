/* The docs pages a space has: those workbench.yaml declares, with lenses
   from its examples implementations, then those defineDocs definitions
   declare, unless the YAML already lists their Markdown file. Shared by the
   server and the portable export. */

import type { DocsLens, DocsPageEntry } from './docs-service.ts';

interface PageItem {
  src: string;
  label: string;
  docs?: boolean;
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
  const declared = items(config?.collections).filter(item => item.docs).map((item): DocsPageEntry => ({
    src: item.src,
    label: item.label,
    lens: item.lens ?? null,
    lenses: Object.keys(item.implementations ?? {}).flatMap((key): DocsLens[] => {
      const impl = config?.implementations?.[key];
      const examples = item.implementations?.[key]?.examples;
      if (!impl || impl.kind !== 'examples' || !impl.adapter || !examples) return [];
      return [{ key, label: impl.label, adapter: impl.adapter, styles: impl.styles ?? [],
        ...(impl.environment ? { environment: impl.environment } : {}), examples }];
    }),
  }));
  const srcs = new Set(declared.map(entry => entry.src));
  return declared.concat(discovered.filter(entry => !srcs.has(entry.src)));
}
