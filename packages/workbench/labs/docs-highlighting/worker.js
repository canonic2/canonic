// Classic, document-owned worker. Dependencies are prebuilt browser files.
let ready;
let engine;
let rules;
let grammarMs = 0;
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
  xml: 'xml',
  html: 'xml',
  svg: 'xml',
  vue: 'xml',
  astro: 'xml',
  yaml: 'yaml',
  yml: 'yaml',
};
const escape = (text) =>
  text.replace(
    /[&<>"']/g,
    (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c],
  );
const scopeNames = {
  bash: 'source.shell',
  css: 'source.css',
  diff: 'source.diff',
  javascript: 'source.js',
  jsx: 'source.js.jsx',
  json: 'source.json',
  markdown: 'text.html.markdown',
  scss: 'source.css.scss',
  typescript: 'source.ts',
  tsx: 'source.tsx',
  xml: 'text.html.basic',
  svg: 'text.xml',
};
function role(scopes) {
  for (const scope of scopes.slice().reverse()) {
    const found = rules.find(
      ([prefix]) =>
        scope === prefix ||
        scope.startsWith(prefix + '.') ||
        (prefix.endsWith('.') && scope.startsWith(prefix)),
    );
    if (found) return found[1] || null;
    if (
      ['meta.embedded', 'meta.template.expression', 'source.', 'text.'].some((prefix) =>
        scope.startsWith(prefix),
      )
    )
      return null;
  }
  return null;
}
async function initialize(variant) {
  const started = performance.now();
  if (variant === '1') {
    const { default: core } = await import('/_workbench/lab-assets/hljs/core.min.js');
    for (const name of [...new Set(Object.values(aliases))]) {
      const { default: language } = await import(
        `/_workbench/lab-assets/hljs/languages/${name}.min.js`
      );
      core.registerLanguage(name, language);
    }
    engine = (text, name) =>
      core.highlight(text, { language: aliases[name], ignoreIllegals: true }).value;
  } else {
    importScripts('/_workbench/lab-assets/onig/main.js', '/_workbench/lab-assets/textmate/main.js');
    const bytes = await fetch('/_workbench/lab-assets/onig/onig.wasm').then((r) => r.arrayBuffer());
    await onig.loadWASM(bytes);
    const index = await fetch('/_workbench/lab-assets/grammars/index.json').then((r) => r.json());
    rules = await fetch('/_workbench/lab-assets/roles.json').then((r) => r.json());
    const registry = new vscodetextmate.Registry({
      onigLib: Promise.resolve({
        createOnigScanner: (patterns) => new onig.OnigScanner(patterns),
        createOnigString: (text) => new onig.OnigString(text),
      }),
      loadGrammar: async (scope) =>
        index[scope]
          ? vscodextmateGrammar(
              await fetch('/_workbench/lab-assets/grammars/' + index[scope]).then((r) => r.text()),
              index[scope],
            )
          : null,
    });
    function vscodextmateGrammar(text, name) {
      return vscodetextmate.parseRawGrammar(text, name);
    }
    const grammars = new Map();
    engine = async (text, name) => {
      const normalized = aliases[name];
      const scope =
        name === 'jsx' || name === 'tsx' || name === 'svg' || name === 'xml'
          ? name === 'xml'
            ? 'text.xml'
            : scopeNames[name]
          : normalized === 'yaml'
            ? 'source.yaml'
            : scopeNames[normalized];
      if (!grammars.has(scope)) {
        const start = performance.now();
        grammars.set(
          scope,
          registry.loadGrammar(scope).then((value) => {
            grammarMs += performance.now() - start;
            return value;
          }),
        );
      }
      const grammar = await grammars.get(scope);
      if (!grammar) throw Error('Missing grammar: ' + scope);
      let stack = vscodetextmate.INITIAL;
      let html = '';
      for (const line of text.match(/[^\n]*\n|[^\n]+$/g) || []) {
        const input = line.endsWith('\n') ? line.slice(0, -1) : line;
        const result = grammar.tokenizeLine(input, stack);
        stack = result.ruleStack;
        for (const token of result.tokens) {
          const value = escape(
            input.slice(token.startIndex, Math.min(input.length, token.endIndex)),
          );
          const kind = role(token.scopes);
          html += kind ? `<span class="${kind}">${value}</span>` : value;
        }
        if (line.endsWith('\n')) html += '\n';
      }
      return html;
    };
  }
  return performance.now() - started;
}
onmessage = async ({ data }) => {
  try {
    const first = !ready;
    if (!ready) ready = initialize(data.variant);
    const initializationMs = first ? await ready : (await ready, 0);
    const grammarBefore = grammarMs;
    const started = performance.now();
    const blocks = [];
    for (const block of data.blocks) {
      const name = String(block.language || '')
        .trim()
        .toLowerCase();
      blocks.push({
        ...block,
        html: aliases[name] ? await engine(block.text, name) : escape(block.text),
      });
    }
    const grammarInitializationMs = grammarMs - grammarBefore;
    postMessage({
      id: data.id,
      blocks,
      initializationMs,
      grammarInitializationMs,
      tokenizationMs: performance.now() - started - grammarInitializationMs,
      resources: performance
        .getEntriesByType('resource')
        .map((e) => ({
          name: new URL(e.name).pathname,
          start: performance.timeOrigin + e.startTime,
          transferSize: e.transferSize,
          decodedBodySize: e.decodedBodySize,
          duration: e.duration,
        })),
    });
  } catch (error) {
    postMessage({ id: data.id, error: String(error.stack || error) });
  }
};
