/* Syntax highlighting for docs pages: fenced code in the Markdown and
   example source for Show code. A fixed set of languages keeps the extension
   small; anything else is shown as escaped text. */

import hljs from 'highlight.js/lib/core';
import bash from 'highlight.js/lib/languages/bash';
import css from 'highlight.js/lib/languages/css';
import diff from 'highlight.js/lib/languages/diff';
import javascript from 'highlight.js/lib/languages/javascript';
import json from 'highlight.js/lib/languages/json';
import markdown from 'highlight.js/lib/languages/markdown';
import scss from 'highlight.js/lib/languages/scss';
import typescript from 'highlight.js/lib/languages/typescript';
import xml from 'highlight.js/lib/languages/xml';
import yaml from 'highlight.js/lib/languages/yaml';

const LANGUAGES = { bash, css, diff, javascript, json, markdown, scss, typescript, xml, yaml };
for (const [name, language] of Object.entries(LANGUAGES)) hljs.registerLanguage(name, language);
hljs.registerAliases(['sh', 'shell', 'zsh'], { languageName: 'bash' });
hljs.registerAliases(['js', 'jsx', 'mjs', 'cjs'], { languageName: 'javascript' });
hljs.registerAliases(['ts', 'tsx', 'mts', 'cts'], { languageName: 'typescript' });
hljs.registerAliases(['html', 'svg', 'vue', 'astro'], { languageName: 'xml' });
hljs.registerAliases(['md'], { languageName: 'markdown' });
hljs.registerAliases(['yml'], { languageName: 'yaml' });

export function escapeHtml(text: string): string {
  return text.replace(/[&<>"']/g, char => '&#' + char.charCodeAt(0) + ';');
}

/** Highlighted HTML for `code`, or the escaped text when the language is unknown. */
export function highlight(code: string, language: string | null | undefined): string {
  const name = (language || '').trim().toLowerCase();
  if (name && hljs.getLanguage(name)) return hljs.highlight(code, { language: name, ignoreIllegals: true }).value;
  return escapeHtml(code);
}

/** The language a source file is highlighted as, from its extension. */
export function languageOf(file: string): string {
  const extension = /\.([a-z0-9]+)$/i.exec(file)?.[1]?.toLowerCase() ?? '';
  return hljs.getLanguage(extension) ? extension : 'plaintext';
}
