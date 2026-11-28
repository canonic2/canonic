import { defineCollection } from 'astro:content';
import { glob } from 'astro/loaders';
import { testrunDocGroups } from './lib/doc-groups.js';

export const collections = {
  workbenchDocs: defineCollection({
    loader: glob({
      base: '../workbench/docs',
      pattern: '**/*.md',
      generateId: ({ entry }) => entry.replace(/\.md$/, '').replace(/(^|\/)README$/, '$1index'),
    }),
  }),
  shieldDocs: defineCollection({
    loader: glob({
      base: '../shield/docs',
      pattern: '**/*.md',
      generateId: ({ entry }) => entry.replace(/\.md$/, '').replace(/(^|\/)README$/, '$1index'),
    }),
  }),
  testrunDocs: defineCollection({
    loader: glob({
      base: '../testrun/docs',
      pattern: testrunDocGroups.flatMap(group => group.ids.map(id => `${id === 'index' ? 'README' : id}.md`)),
      generateId: ({ entry }) => entry.replace(/\.md$/, '').replace(/(^|\/)README$/, '$1index'),
    }),
  }),
};
