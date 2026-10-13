/* Rspack pipeline lab: turns an HTML source (an HTML preview, or an Astro
   render) into a module whose default export is the page's HTML, as the
   esbuild HTML loader does. Linked local stylesheets become imports, so Rspack
   bundles them and replaces them in place on edit. Local images and media
   become asset URLs. Inline scripts and styles stay in the HTML, which the
   HTML adapter runs on each render. `wbfile:<absolute path>` names a file the
   worker generated (Astro's styles and scripts). */
const path = require('node:path');

const external = value => /^(?:[a-z][a-z0-9+.-]*:|\/\/|#)/i.test(value) && !value.startsWith('wbfile:');

module.exports = function htmlLoader(source) {
  const root = this.rootContext;
  const request = value => {
    const clean = value.split(/[?#]/)[0];
    if (clean.startsWith('wbfile:')) return clean.slice('wbfile:'.length);
    if (clean.startsWith('/')) return path.join(root, clean.slice(1));
    return path.resolve(path.dirname(this.resourcePath), clean);
  };
  const imports = [];
  const urls = [];
  let html = source;
  html = html.replace(/<link\b[^>]*>/gi, tag => {
    if (!/\brel\s*=\s*["']?stylesheet/i.test(tag)) return tag;
    const href = /\bhref\s*=\s*(["'])([^"']+)\1/i.exec(tag);
    if (!href || external(href[2])) return tag;
    imports.push('import ' + JSON.stringify(request(href[2])) + ';');
    return '';
  });
  // Astro hoists its client scripts into module scripts that import a resource.
  html = html.replace(/<script\b[^>]*type\s*=\s*["']module["'][^>]*>\s*import\s*["'](wbfile:[^"']+)["'];?\s*<\/script>/gi, (_, value) => {
    imports.push('import ' + JSON.stringify(request(value)) + ';');
    return '';
  });
  const placeholder = value => { urls.push(request(value)); return '__WB_URL_' + (urls.length - 1) + '__'; };
  html = html.replace(/\b(src|poster)\s*=\s*(["'])([^"']+)\2/gi, (match, attribute, quote, value) =>
    external(value) || /<script\b/i.test(match) ? match : attribute + '=' + quote + placeholder(value) + quote);
  html = html.replace(/\bsrcset\s*=\s*(["'])([^"']+)\1/gi, (match, quote, value) => 'srcset=' + quote + value.split(',').map(item => {
    const [url, ...size] = item.trim().split(/\s+/);
    return (external(url) ? url : placeholder(url)) + (size.length ? ' ' + size.join(' ') : '');
  }).join(', ') + quote);
  html = html.replace(/<title>[\s\S]*?<\/title>|<meta\b[^>]*>|<base\b[^>]*>/gi, '');
  const parts = html.split(/__WB_URL_(\d+)__/);
  const pieces = parts.map((part, index) => index % 2 ? 'url' + part : JSON.stringify(part));
  return imports.join('\n') + '\n' +
    urls.map((url, index) => 'import url' + index + ' from ' + JSON.stringify(url) + ';').join('\n') + '\n' +
    'export default ' + pieces.join(' + ') + ';\n';
};
