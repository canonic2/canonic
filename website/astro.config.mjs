import { defineConfig } from 'astro/config';

// GitHub Pages serves the site under the repository's path. The publish
// workflow passes the origin and base path that actions/configure-pages
// reports; the defaults match canonic2/canonic without a custom domain.
export default defineConfig({
  site: process.env.SITE_ORIGIN || 'https://canonic2.github.io',
  base: (process.env.SITE_BASE ?? '/canonic') || '/',
});
