// Workbench previews of this site. The docs pages read product guides
// through astro:content, which needs Astro's application pipeline, so an
// alias stands in for it: previews/content.ts renders every guide with
// Astro's own Markdown processor and the site's plugins.
import { defineConfig } from '@canonic2/workbench';

export default defineConfig({
  aliases: { 'astro:content': './previews/content.ts' },
});
