import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';

export const collections = {
  workbenchDocs: defineCollection({
    loader: glob({
      base: '../workbench/docs',
      pattern: '**/*.md',
      generateId: ({ entry }) => entry.replace(/\.md$/, '').replace(/(^|\/)README$/, '$1index'),
    }),
  }),
};
