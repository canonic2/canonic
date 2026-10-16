/* The source text of one named export, for **Show code** on a single-file
   example source. A whole-file example shows its file; a named export shows
   only its own statement, with the comment and decorators directly above it.

   The statement is found by layout, not by parsing: it starts at its
   column-0 `export` (or, for `export { local as name }`, the column-0
   declaration of `local`) and ends before the next column-0 line that starts a
   new statement. Formatted source keeps every top-level statement at column
   0 and everything inside one indented or closing (`}`, `)`, `]`), so this
   holds without a TypeScript and JSX parser, which a lexer can't safely
   approximate: an apostrophe in JSX text is not a string quote. A multi-line
   template literal whose lines start at column 0 can end a statement early. */

const CLOSER = /^[}\])]/;

export function exportSource(source: string, name: string): string | null {
  const lines = source.replace(/\r\n?/g, '\n').split('\n');
  const start = findDeclaration(lines, name);
  if (start === null) return null;
  if (start.reexport) return lines[start.line]!;
  const first = leadingStart(lines, start.line);
  const last = statementEnd(lines, start.line);
  return lines.slice(first, last + 1).join('\n');
}

function escape(name: string): string {
  return name.replace(/[$]/g, '\\$');
}

function findDeclaration(lines: string[], name: string): { line: number; reexport: boolean } | null {
  const id = escape(name);
  const direct = new RegExp(
    `^export\\s+(?:declare\\s+)?(?:(?:async\\s+)?function\\*?\\s+${id}\\b|(?:const|let|var)\\s+${id}\\b|(?:abstract\\s+)?class\\s+${id}\\b)`,
  );
  for (let index = 0; index < lines.length; index++) {
    if (direct.test(lines[index]!)) return { line: index, reexport: false };
  }

  const listed = exportList(lines, name);
  if (!listed) return null;
  if (listed.from) return { line: listed.line, reexport: true };
  const local = escape(listed.local);
  const declaration = new RegExp(`^(?:(?:async\\s+)?function\\*?\\s+${local}\\b|(?:const|let|var)\\s+${local}\\b|(?:abstract\\s+)?class\\s+${local}\\b)`);
  for (let index = 0; index < lines.length; index++) {
    if (declaration.test(lines[index]!)) return { line: index, reexport: false };
  }
  return null;
}

/** `export { local as name }` or `export { name } from '…'`, which may span lines. */
function exportList(lines: string[], name: string): { line: number; local: string; from: boolean } | null {
  for (let index = 0; index < lines.length; index++) {
    if (!/^export\s*(?:type\s*)?\{/.test(lines[index]!)) continue;
    let text = lines[index]!;
    let end = index;
    while (!text.includes('}') && end + 1 < lines.length) text += '\n' + lines[++end];
    const brace = /\{([^}]*)\}\s*(from\b)?/.exec(text);
    if (!brace) continue;
    for (const part of brace[1]!.split(',')) {
      const match = /^\s*(?:type\s+)?([\w$]+)(?:\s+as\s+([\w$]+))?\s*$/.exec(part);
      if (match && (match[2] ?? match[1]) === name) return { line: index, local: match[1]!, from: !!brace[2] };
    }
  }
  return null;
}

/** The first line of the comment block and decorators directly above a statement. */
function leadingStart(lines: string[], line: number): number {
  let first = line;
  while (first > 0) {
    const above = lines[first - 1]!;
    if (/^\s*(\/\/|\/\*|\*|@)/.test(above) && above.trim()) first--;
    else break;
  }
  return first;
}

function statementEnd(lines: string[], line: number): number {
  let last = line;
  for (let index = line + 1; index < lines.length; index++) {
    const text = lines[index]!;
    if (!text.trim()) continue;
    if (/^\s/.test(text) || CLOSER.test(text)) {
      last = index;
      continue;
    }
    break;
  }
  return last;
}
