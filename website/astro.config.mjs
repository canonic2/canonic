import { defineConfig } from 'astro/config';
import { docsMarkdown, shikiConfig } from './src/lib/docs.js';

// The publish workflow passes the origin and base path reported by
// actions/configure-pages; local builds default to the custom domain.
export default defineConfig({
  site: process.env.SITE_ORIGIN || 'https://canonic.sh',
  base: process.env.SITE_BASE || '/',
  integrations: [docsMarkdown()],
  markdown: { shikiConfig },
});
