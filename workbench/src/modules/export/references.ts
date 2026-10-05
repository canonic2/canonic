export function packageName(specifier: string): string | null {
  if (!specifier || specifier.charAt(0) === '.' || specifier.charAt(0) === '/' || specifier.charAt(0) === '#') return null;
  if (/^[a-z][a-z0-9+.-]*:/i.test(specifier)) return null;
  var parts = specifier.split('/');
  return specifier.charAt(0) === '@' ? parts.slice(0, 2).join('/') : parts[0];
}

function withoutComments(source: string) {
  var out = '';
  var quote = null;
  for (var i = 0; i < source.length; i += 1) {
    var char = source[i];
    var next = source[i + 1];
    if (quote) {
      out += char;
      if (char === '\\') { if (next) out += source[++i]; }
      else if (char === quote) quote = null;
      continue;
    }
    if (char === '"' || char === "'" || char === '`') { quote = char; out += char; continue; }
    if (char === '/' && next === '/') {
      while (i < source.length && source[i] !== '\n') i += 1;
      out += '\n';
      continue;
    }
    if (char === '/' && next === '*') {
      i += 2;
      while (i < source.length && !(source[i] === '*' && source[i + 1] === '/')) i += 1;
      i += 1;
      out += ' ';
      continue;
    }
    out += char;
  }
  return out;
}

/** Parses static references without filesystem or host dependencies. */
export function parseReferences(ext: string, body: string): string[] {
  var out: string[] = [];
  function take(pattern: RegExp) {
    var match;
    while ((match = pattern.exec(body))) if (out.indexOf(match[1]) === -1) out.push(match[1]);
  }
  if (['.js', '.jsx', '.ts', '.tsx', '.mjs', '.cjs'].indexOf(ext) > -1) {
    body = withoutComments(body);
    take(/(?:import|export)\s+(?:type\s+)?(?:[^"'`;]*?\s+from\s+)?["']([^"']+)["']/g);
    take(/(?:import|require)\s*\(\s*["']([^"']+)["']\s*\)/g);
    take(/new\s+URL\s*\(\s*["']([^"']+)["']\s*,\s*import\.meta\.url\s*\)/g);
  }
  if (['.css', '.scss', '.sass', '.less'].indexOf(ext) > -1) {
    take(/@(?:import|use|forward)\s+(?:url\(\s*)?["']([^"']+)["']/g);
    take(/url\(\s*["']?([^"')]+)["']?\s*\)/g);
  }
  if (ext === '.html' || ext === '.htm') take(/(?:src|href)\s*=\s*["']([^"'#]+)["']/gi);
  return out;
}

export function stylesheetImports(body: string) {
  var imports = new Set<string>();
  var pattern = /@(?:import|use|forward)\s+(?:url\(\s*)?["']([^"']+)["']/g;
  var match;
  while ((match = pattern.exec(body))) imports.add(match[1]);
  return imports;
}
