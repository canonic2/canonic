// Local, explicitly registered grammars. This module runs only in a worker.
import hljs from '../../../vendor/highlight.js/core.min.js';
import bash from '../../../vendor/highlight.js/languages/bash.min.js';
import css from '../../../vendor/highlight.js/languages/css.min.js';
import diff from '../../../vendor/highlight.js/languages/diff.min.js';
import javascript from '../../../vendor/highlight.js/languages/javascript.min.js';
import json from '../../../vendor/highlight.js/languages/json.min.js';
import markdown from '../../../vendor/highlight.js/languages/markdown.min.js';
import scss from '../../../vendor/highlight.js/languages/scss.min.js';
import typescript from '../../../vendor/highlight.js/languages/typescript.min.js';
import xml from '../../../vendor/highlight.js/languages/xml.min.js';
import yaml from '../../../vendor/highlight.js/languages/yaml.min.js';

for (const [name, grammar] of Object.entries({
  bash,
  css,
  diff,
  javascript,
  json,
  markdown,
  scss,
  typescript,
  xml,
  yaml,
}))
  hljs.registerLanguage(name, grammar);

const aliases = {
  bash: 'bash',
  sh: 'bash',
  shell: 'bash',
  zsh: 'bash',
  css: 'css',
  diff: 'diff',
  javascript: 'javascript',
  js: 'javascript',
  jsx: 'javascript',
  mjs: 'javascript',
  cjs: 'javascript',
  json: 'json',
  markdown: 'markdown',
  md: 'markdown',
  scss: 'scss',
  typescript: 'typescript',
  ts: 'typescript',
  tsx: 'typescript',
  mts: 'typescript',
  cts: 'typescript',
  html: 'xml',
  xml: 'xml',
  svg: 'xml',
  vue: 'xml',
  astro: 'xml',
  yaml: 'yaml',
  yml: 'yaml',
};
const escape = (text) =>
  text.replace(
    /[&<>"']/g,
    (character) =>
      ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[character],
  );

self.addEventListener('message', ({ data }) => {
  try {
    const blocks = data.blocks.map(({ text, language }) => {
      const key = String(language || '')
        .trim()
        .toLowerCase();
      const name = Object.hasOwn(aliases, key) ? aliases[key] : undefined;
      return {
        text,
        language,
        html: name
          ? hljs.highlight(text, { language: name, ignoreIllegals: true }).value
          : escape(text),
      };
    });
    self.postMessage({ id: data.id, blocks });
  } catch (error) {
    self.postMessage({ id: data.id, error: String(error.message || error) });
  }
});
