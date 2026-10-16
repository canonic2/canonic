// Pure response boundary and discovery ordering. Directory entries are compared
// as the compiler's depth-first walk compares them, including folder prefixes.
export type Entry = { id: string; file: string; [key: string]: any };
export type IndexAnswer = { previews: Entry[]; docs: Entry[]; errors: string[] };
type Record = { file: string; docs: boolean; entry: { Ok: string } | { Err: string } };

function orderRecords(records: Record[]): Record[] {
  type Tree = Map<string, Tree | Record>;
  const root: Tree = new Map();
  for (const record of records) {
    const segments = record.file.split('/');
    let tree = root;
    for (const segment of segments.slice(0, -1)) {
      let child = tree.get(segment);
      if (!child) { child = new Map(); tree.set(segment, child); }
      if (!(child instanceof Map)) throw new Error('Invalid native index path');
      tree = child;
    }
    tree.set(segments.at(-1)!, record);
  }
  const ordered: Record[] = [];
  function walk(tree: Tree) {
    for (const [, item] of [...tree].sort(([a], [b]) => a.localeCompare(b))) {
      if (item instanceof Map) walk(item);
      else ordered.push(item);
    }
  }
  walk(root);
  return ordered;
}

export function discoveryOrder(left: string, right: string): number {
  const a = left.split('/');
  const b = right.split('/');
  for (let at = 0; at < Math.min(a.length, b.length); at++) {
    const order = a[at].localeCompare(b[at]);
    if (order) return order;
  }
  return a.length - b.length;
}

export function readAnswer(raw: unknown): { answer: IndexAnswer; timings: { [key: string]: unknown } } {
  if (!raw || typeof raw !== 'object' || !('records' in raw) || !Array.isArray(raw.records) || !('timings' in raw) || !raw.timings || typeof raw.timings !== 'object') throw new Error('Invalid native page index response');
  const records: Record[] = raw.records.map(record => {
    if (!record || typeof record.file !== 'string' || typeof record.docs !== 'boolean' || !record.entry || typeof record.entry !== 'object' || !('Ok' in record.entry || 'Err' in record.entry)) throw new Error('Invalid native page index record');
    return record;
  });
  const answer: IndexAnswer = { previews: [], docs: [], errors: [] };
  const ids = new Set<string>();
  for (const record of orderRecords(records)) {
    if ('Err' in record.entry) {
      if (typeof record.entry.Err !== 'string') throw new Error('Invalid native page index error');
      answer.errors.push(record.file + ': ' + record.entry.Err);
      continue;
    }
    const entry = JSON.parse(record.entry.Ok);
    if (!entry || typeof entry.id !== 'string' || entry.file !== record.file || !Array.isArray(entry.states) || (record.docs ? typeof entry.src !== 'string' || !Array.isArray(entry.lenses) : typeof entry.adapter !== 'string' || typeof entry.source !== 'string')) throw new Error('Invalid native page index entry');
    if (ids.has(entry.id)) { answer.errors.push(record.file + ': Duplicate preview id: ' + entry.id); continue; }
    ids.add(entry.id);
    (record.docs ? answer.docs : answer.previews).push(entry);
  }
  return { answer, timings: raw.timings as { [key: string]: unknown } };
}
