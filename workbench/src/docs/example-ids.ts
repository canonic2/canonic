/* Example IDs name an example in Markdown placements, sidebar rows, and
   addresses. They are kebab-case, like state IDs. A folder source derives them
   from file names; a single-file source from its named exports. */

const KEBAB = /^[a-z0-9]+(?:-[a-z0-9]+)*$/;

/** Script extensions an adapter compiles as an example file. */
const EXTENSIONS: Record<string, readonly string[]> = {
  html: ['.ts', '.tsx', '.js', '.jsx', '.mjs'],
  react: ['.ts', '.tsx', '.js', '.jsx', '.mjs'],
  'react-native-web': ['.ts', '.tsx', '.js', '.jsx', '.mjs'],
  vue: ['.vue', '.ts', '.tsx', '.js', '.jsx', '.mjs'],
  astro: ['.astro'],
};

const DEFAULT_EXTENSIONS: readonly string[] = ['.ts', '.tsx', '.js', '.jsx', '.mjs'];

export function isExampleId(value: string): boolean {
  return KEBAB.test(value);
}

/** `withCustomStyle` → `with-custom-style`; `Basic` → `basic`; `size2x` → `size2x`. */
export function exportNameToId(name: string): string {
  return name
    .replace(/([a-z0-9])([A-Z])/g, '$1-$2')
    .replace(/([A-Z]+)([A-Z][a-z])/g, '$1-$2')
    .replace(/_+/g, '-')
    .toLowerCase();
}

/** The extensions a folder source considers for an adapter. Project-registered adapters get the script defaults. */
export function exampleExtensions(adapter: string): readonly string[] {
  return EXTENSIONS[adapter] ?? DEFAULT_EXTENSIONS;
}

export interface FolderEntry {
  /** The file name inside the folder, such as `with-custom-style.tsx`. */
  name: string;
  isFile: boolean;
}

export interface FolderExample {
  id: string;
  /** The file name inside the folder. */
  file: string;
}

export interface FolderExamples {
  examples: FolderExample[];
  /** File names that would be examples but are not kebab-case, or that share an ID. */
  rejected: { file: string; reason: 'not-kebab-case' | 'duplicate-id' }[];
}

/**
 * The examples a folder holds: each file directly inside it whose extension the
 * adapter compiles, by file name order. Test files, declaration files, and
 * dot-files are not examples.
 */
export function examplesInFolder(entries: readonly FolderEntry[], adapter: string): FolderExamples {
  const extensions = exampleExtensions(adapter);
  const examples: FolderExample[] = [];
  const rejected: FolderExamples['rejected'] = [];
  const seen = new Set<string>();
  const names = entries
    .filter(entry => entry.isFile && !entry.name.startsWith('.'))
    .map(entry => entry.name)
    .sort((a, b) => a.localeCompare(b));
  for (const name of names) {
    if (/\.d\.[cm]?ts$/.test(name) || /\.(test|spec)\.[^.]+$/.test(name)) continue;
    const extension = extensions.find(candidate => name.endsWith(candidate));
    if (!extension) continue;
    const id = name.slice(0, -extension.length);
    if (!isExampleId(id)) rejected.push({ file: name, reason: 'not-kebab-case' });
    else if (seen.has(id)) rejected.push({ file: name, reason: 'duplicate-id' });
    else {
      seen.add(id);
      examples.push({ id, file: name });
    }
  }
  return { examples, rejected };
}
