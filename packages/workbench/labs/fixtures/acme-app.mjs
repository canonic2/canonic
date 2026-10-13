// Deterministic Acme project generator shared by Workbench labs. It writes a
// project with workbench.yaml, workbench.config.ts, a React environment and
// TypeScript previews, shaped like a large web app. See README.md.
import fs from 'node:fs';
import path from 'node:path';
import zlib from 'node:zlib';

// Pinned at least two weeks before the labs' plans (2026-10-11).
export const DEPENDENCIES = {
  small: {
    react: '19.3.0',
    'react-dom': '19.3.0',
  },
  large: {
    react: '19.3.0',
    'react-dom': '19.3.0',
    '@apollo/client': '4.3.1',
    graphql: '17.0.2',
    rxjs: '7.8.2',
    'lodash-es': '4.18.1',
    'date-fns': '4.4.0',
    'libphonenumber-js': '1.13.14',
    'i18n-iso-countries': '7.14.0',
    // v9 replaced the table hook API; v8 is the widely deployed one.
    '@tanstack/react-table': '8.21.3',
    '@floating-ui/react': '0.27.20',
    zustand: '5.0.15',
    '@fontsource/inter': '5.3.0',
  },
};
// The large shapes share the large lockfile, apart from large-web's fonts.
DEPENDENCIES['large-system'] = DEPENDENCIES.large;
DEPENDENCIES.huge = DEPENDENCIES.large;
DEPENDENCIES['large-web'] = {
  ...DEPENDENCIES.large,
  '@fontsource-variable/inter': '5.3.0',
  '@fontsource/noto-sans-jp': '5.3.0',
  '@fontsource-variable/material-symbols-outlined': '5.3.8',
  '@fontsource/jetbrains-mono': '5.3.0',
};
export const LOCKFILE = { small: 'small', large: 'large', 'large-system': 'large', huge: 'large', 'large-web': 'large-web' };

const SIZES = {
  small: { components: 5, features: 0, pages: 0, componentBytes: 2500 },
  large: { components: 140, features: 60, pages: 40, componentBytes: 6000, sections: [14, 34], fonts: 'inter' },
  // The Rspack pipeline lab's shapes: the large app with system fonts only,
  // with heavy web fonts and a large photo per page, and with 200 pages. Each
  // page also leaves the extended isolation sentinels behind.
  'large-system': { components: 140, features: 60, pages: 40, componentBytes: 6000, sections: [14, 34], fonts: 'system', sentinels: true },
  'large-web': { components: 140, features: 60, pages: 40, componentBytes: 6000, sections: [14, 34], fonts: 'web', photos: true, sentinels: true },
  huge: { components: 140, features: 60, pages: 200, componentBytes: 6000, sections: [14, 34], fonts: 'system', sentinels: true },
};

const NOUNS = ['job', 'quote', 'invoice', 'client', 'site', 'visit', 'crew', 'asset', 'part', 'estimate',
  'schedule', 'payment', 'contract', 'route', 'ticket', 'lead', 'vendor', 'expense', 'timesheet', 'report',
  'project', 'task', 'note', 'photo', 'form', 'checklist', 'permit', 'warranty', 'region', 'team'];
const ADJECTIVES = ['active', 'pending', 'archived', 'recent', 'urgent', 'shared', 'draft', 'overdue', 'paid',
  'scheduled', 'assigned', 'open', 'closed', 'flagged', 'billable'];
const UI = ['Button', 'Badge', 'Card', 'Panel', 'Field', 'Select', 'Tabs', 'Menu', 'Dialog', 'Tooltip', 'Avatar',
  'Table', 'List', 'Toolbar', 'Banner', 'Stepper', 'Chip', 'Toggle', 'Slider', 'Popover', 'Drawer', 'Pager',
  'Stat', 'Meter', 'Timeline', 'Calendar', 'Upload', 'Search', 'Filter', 'Breadcrumb'];
const LODASH = ['sortBy', 'groupBy', 'uniqBy', 'chunk', 'debounce', 'throttle', 'pick', 'omit', 'merge',
  'cloneDeep', 'isEqual', 'sumBy', 'meanBy', 'partition', 'keyBy', 'orderBy', 'flatMap', 'startCase', 'camelCase', 'kebabCase'];
const DATE_FNS = ['format', 'addDays', 'subDays', 'differenceInDays', 'isAfter', 'isBefore', 'startOfWeek',
  'endOfMonth', 'formatDistanceStrict', 'parseISO'];
const LOCALES = ['de', 'en', 'es', 'fr'];
const FIXED_DATE = '2026-03-14T09:30:00.000Z';

function random(seed) {
  let a = seed >>> 0;
  return () => {
    a = (a + 0x6D2B79F5) >>> 0;
    let t = a;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
  };
}

const pascal = text => text.split(/[-\s]/).map(part => part[0].toUpperCase() + part.slice(1)).join('');
const camel = text => { const p = pascal(text); return p[0].toLowerCase() + p.slice(1); };

class Writer {
  constructor(root) { this.root = root; this.files = 0; this.bytes = 0; }
  write(file, contents) {
    const target = path.join(this.root, file);
    fs.mkdirSync(path.dirname(target), { recursive: true });
    fs.writeFileSync(target, contents);
    this.files++;
    this.bytes += Buffer.byteLength(contents);
  }
}

// Plain TypeScript helpers sized to `bytes`: arithmetic, string and array work
// a real module would carry, with names unique to the module.
function helpers(rand, prefix, bytes) {
  const out = [];
  let size = 0;
  for (let index = 0; size < bytes; index++) {
    const noun = NOUNS[Math.floor(rand() * NOUNS.length)];
    const adjective = ADJECTIVES[Math.floor(rand() * ADJECTIVES.length)];
    const factor = 2 + Math.floor(rand() * 17);
    const block = `
export function ${prefix}${pascal(adjective)}${pascal(noun)}${index}(values: number[], label = '${adjective} ${noun}'): { label: string; total: number; scaled: number[]; summary: string } {
  const scaled = values.map((value, position) => Math.round((value * ${factor} + position) / ${factor - 1}));
  const total = scaled.reduce((sum, value) => sum + value, 0);
  const summary = total > ${factor * 10} ? label.toUpperCase() + ' ' + total.toFixed(1) : label + ': ' + scaled.join(', ');
  return { label, total, scaled, summary };
}
`;
    out.push(block);
    size += block.length;
  }
  return out.join('');
}

function component(rand, name, { bytes, imports = [], lodash = [], dates = [], tooltip = false }) {
  const lines = ["import { useMemo, useState } from 'react';"];
  if (lodash.length) lines.push(`import { ${lodash.join(', ')} } from 'lodash-es';`);
  if (dates.length) lines.push(`import { ${dates.join(', ')} } from 'date-fns';`);
  if (tooltip) lines.push("import { useFloating, offset, flip } from '@floating-ui/react';");
  for (const item of imports) lines.push(`import { ${item.name} } from '${item.from}';`);
  const labels = Array.from({ length: 12 }, (_, index) =>
    `  ${camel(NOUNS[(index * 7 + name.length) % NOUNS.length])}${index}: '${pascal(ADJECTIVES[index % ADJECTIVES.length])} ${NOUNS[index % NOUNS.length]}',`).join('\n');
  const uses = [
    lodash.includes('sortBy') ? "const ordered = useMemo(() => sortBy(items, 'value'), [items]);" : 'const ordered = useMemo(() => [...items].sort((a, b) => a.value - b.value), [items]);',
    dates.includes('format') ? `const stamp = format(new Date('${FIXED_DATE}'), 'yyyy-MM-dd');` : `const stamp = '${FIXED_DATE.slice(0, 10)}';`,
    tooltip ? 'const floating = useFloating({ middleware: [offset(4), flip()] });' : 'const floating = null;',
  ];
  const children = imports.slice(0, 2).map(item => `      <${item.name} />`).join('\n');
  const body = `
export interface ${name}Props {
  title?: string;
  items?: Array<{ id: string; label: string; value: number }>;
  tone?: 'neutral' | 'accent' | 'danger';
}

const ${camel(name)}Labels = {
${labels}
};

export function ${name}({ title = '${name}', items = [], tone = 'neutral' }: ${name}Props) {
  const [open, setOpen] = useState(false);
  ${uses.join('\n  ')}
  const labels = Object.values(${camel(name)}Labels);
  return (
    <section className={'acme-${name.toLowerCase()} acme-tone-' + tone} data-component="${name}" ref={floating ? floating.refs.setReference : undefined}>
      <header>
        <h3>{title}</h3>
        <button type="button" onClick={() => setOpen(!open)}>{open ? 'Hide' : 'Show'} {labels.length}</button>
        <time dateTime={stamp}>{stamp}</time>
      </header>
      {open && <ul>{ordered.map(item => <li key={item.id}>{item.label}: {item.value}</li>)}</ul>}
${children}
    </section>
  );
}
`;
  const prefix = lines.join('\n') + '\n' + body;
  return prefix + helpers(rand, camel(name), Math.max(0, bytes - prefix.length));
}

function generateUi(writer, rand, count, bytes, rich) {
  const names = [];
  for (let index = 0; index < count; index++) {
    const name = UI[index % UI.length] + (index >= UI.length ? Math.floor(index / UI.length) + 1 : '');
    const imports = names.length && rand() < 0.5 ? [{ name: names[Math.floor(rand() * names.length)], from: './' + names[Math.floor(rand() * names.length)] }] : [];
    // Re-pick so the import and its file agree.
    if (imports.length) imports[0].from = './' + imports[0].name;
    writer.write(`src/ui/${name}.tsx`, component(rand, name, {
      bytes, imports,
      lodash: rich ? LODASH.filter(() => rand() < 0.12).concat('sortBy').filter((v, i, a) => a.indexOf(v) === i) : [],
      dates: rich ? ['format'] : [],
      tooltip: rich && name.startsWith('Tooltip'),
    }));
    names.push(name);
  }
  return names;
}

function generateFeatures(writer, rand, count, ui) {
  const features = [];
  for (let index = 0; index < count; index++) {
    const noun = NOUNS[index % NOUNS.length];
    const slug = noun + (index >= NOUNS.length ? '-' + (Math.floor(index / NOUNS.length) + 1) : '');
    const Name = pascal(slug);
    const lodash = LODASH.filter(() => rand() < 0.2).concat('groupBy').filter((v, i, a) => a.indexOf(v) === i);
    const dates = DATE_FNS.filter(() => rand() < 0.3).concat('format').filter((v, i, a) => a.indexOf(v) === i);
    writer.write(`src/app/features/${slug}/model.ts`, `import { ${lodash.join(', ')} } from 'lodash-es';
import { ${dates.join(', ')} } from 'date-fns';

export interface ${Name}Record { id: string; label: string; value: number; updatedAt: string }

export function group${Name}s(records: ${Name}Record[]) {
  return groupBy(records, record => record.value % 3);
}

export function describe${Name}(record: ${Name}Record) {
  return record.label + ' — ' + format(new Date(record.updatedAt), 'PP');
}
${helpers(rand, camel(slug) + 'Model', 20000)}`);
    writer.write(`src/app/features/${slug}/queries.ts`, `import { gql } from '@apollo/client';

export const ${Name}ListQuery = gql\`
  query ${Name}ListQuery($first: Int) {
    items(first: $first) { id label value updatedAt }
  }
\`;

export const ${Name}DetailQuery = gql\`
  query ${Name}DetailQuery($id: ID!) {
    item(id: $id) { id label value updatedAt notes }
  }
\`;
`);
    const widgetUi = [ui[index % ui.length], ui[(index * 3 + 1) % ui.length]];
    writer.write(`src/app/features/${slug}/widgets.tsx`, component(rand, `${Name}Widget`, {
      bytes: 24000,
      imports: widgetUi.map(name => ({ name, from: `../../../ui/${name}` })),
      lodash: ['sortBy'], dates: ['format'],
    }));
    writer.write(`src/app/features/${slug}/index.ts`, `export * from './model';
export * from './queries';
export * from './widgets';
export const title = '${pascal(slug).replace(/([a-z])([A-Z0-9])/g, '$1 $2')}';
export const slug = '${slug}';
`);
    features.push(slug);
  }
  // The shell registers every feature eagerly, as an app's route tree does.
  writer.write('src/app/registry.ts', features.map((slug, index) => `import * as feature${index} from './features/${slug}';`).join('\n') +
    `\n\nexport const features = [${features.map((_, index) => `feature${index}`).join(', ')}];\n`);
  writer.write('src/app/Shell.tsx', `import type { ReactNode } from 'react';
import { features } from './registry';

export function Shell({ children }: { children: ReactNode }) {
  return (
    <div className="acme-shell">
      <nav aria-label="Features">{features.map(feature => <a key={feature.slug} href={'#' + feature.slug}>{feature.title}</a>)}</nav>
      <main>{children}</main>
    </div>
  );
}
`);
  return features;
}

const svg = (rand, label) => {
  const hue = Math.floor(rand() * 360);
  return `<svg xmlns="http://www.w3.org/2000/svg" width="640" height="320" viewBox="0 0 640 320"><rect width="640" height="320" fill="hsl(${hue} 60% 85%)"/><circle cx="${100 + Math.floor(rand() * 440)}" cy="160" r="${40 + Math.floor(rand() * 80)}" fill="hsl(${(hue + 120) % 360} 55% 55%)"/><text x="24" y="296" font-family="sans-serif" font-size="28">${label}</text></svg>\n`;
};

function records(rand, count) {
  return Array.from({ length: count }, (_, index) => ({
    __typename: 'Item', id: 'item-' + index,
    label: pascal(ADJECTIVES[Math.floor(rand() * ADJECTIVES.length)]) + ' ' + NOUNS[Math.floor(rand() * NOUNS.length)],
    value: Math.floor(rand() * 1000), updatedAt: FIXED_DATE,
  }));
}

// A deterministic photo-sized PNG: a gradient with noise, so it compresses
// about as poorly as an unoptimized hero photo (about 2.5 MB).
function photo(seed) {
  const width = 1200, height = 800;
  const rand = random(seed);
  const raw = Buffer.alloc((width * 3 + 1) * height);
  const hue = rand() * 255;
  for (let y = 0, offset = 0; y < height; y++) {
    raw[offset++] = 0;
    for (let x = 0; x < width; x++) {
      const noise = Math.floor(rand() * 12);
      raw[offset++] = (hue + x / 6 + noise) & 255;
      raw[offset++] = (y / 4 + noise) & 255;
      raw[offset++] = (255 - x / 5 + noise) & 255;
    }
  }
  const chunk = (type, data) => {
    const length = Buffer.alloc(4); length.writeUInt32BE(data.length);
    const body = Buffer.concat([Buffer.from(type), data]);
    const crc = Buffer.alloc(4); crc.writeUInt32BE(zlib.crc32(body));
    return Buffer.concat([length, body, crc]);
  };
  const header = Buffer.alloc(13);
  header.writeUInt32BE(width, 0); header.writeUInt32BE(height, 4); header[8] = 8; header[9] = 2;
  return Buffer.concat([Buffer.from([137, 80, 78, 71, 13, 10, 26, 10]), chunk('IHDR', header), chunk('IDAT', zlib.deflateSync(raw, { level: 6 })), chunk('IEND', Buffer.alloc(0))]);
}

function generatePages(writer, rand, count, ui, features, sectionRange, shape = {}) {
  const pages = [];
  for (let index = 0; index < count; index++) {
    const slug = NOUNS[index % NOUNS.length] + 's' + (index >= NOUNS.length ? '-' + (Math.floor(index / NOUNS.length) + 1) : '');
    const Name = pascal(slug);
    const dir = `src/pages/${slug}`;
    const sections = sectionRange[0] + Math.floor(rand() * (sectionRange[1] - sectionRange[0] + 1));
    for (let section = 0; section < sections; section++) {
      writer.write(`${dir}/sections/Section${section}.tsx`, component(rand, `${Name}Section${section}`, {
        bytes: 5000 + Math.floor(rand() * 6000),
        imports: [{ name: ui[(index + section) % ui.length], from: `../../../ui/${ui[(index + section) % ui.length]}` }],
        lodash: LODASH.filter(() => rand() < 0.1), dates: rand() < 0.3 ? ['format'] : [],
      }));
    }
    writer.write(`${dir}/assets/hero.svg`, svg(rand, Name));
    writer.write(`${dir}/assets/badge.svg`, svg(rand, Name + ' badge'));
    if (shape.photos) writer.write(`${dir}/assets/photo.png`, photo(1000 + index));
    // Some web-font pages use a monospace family no other page loads.
    const mono = shape.fonts === 'web' && index % 4 === 3;
    writer.write(`${dir}/page.css`, `.acme-page-${slug} { --acme-page: '${slug}'; padding: 24px; }\n.acme-page-${slug} h1 { font-weight: 600; }\n`);
    writer.write(`${dir}/Interactive.tsx`, `import { useState } from 'react';

export function Interactive() {
  const [count, setCount] = useState(0);
  const [note, setNote] = useState('');
  return (
    <aside className="acme-interactive">
      <button type="button" data-acme="counter" onClick={() => setCount(count + 1)}>Count {count}</button>
      <label>Note <input data-acme="note" value={note} onChange={event => setNote(event.target.value)} /></label>
      <p data-acme="edit-marker">Revision 0</p>
    </aside>
  );
}
`);
    const pageFeatures = [features[index % features.length], features[(index * 7 + 3) % features.length]];
    const table = index % 2 === 0;
    const phone = index % 3 === 0;
    const locale = LOCALES[index % LOCALES.length];
    const sectionImports = Array.from({ length: sections }, (_, section) => `import { ${Name}Section${section} } from './sections/Section${section}';`).join('\n');
    writer.write(`${dir}/Page.tsx`, `import { useEffect, useState } from 'react';
import { useQuery } from '@apollo/client/react';
${table ? "import { createColumnHelper, flexRender, getCoreRowModel, useReactTable } from '@tanstack/react-table';\n" : ''}${phone ? "import { parsePhoneNumberFromString } from 'libphonenumber-js/max';\n" : ''}import countries from 'i18n-iso-countries';
import { ${pascal(pageFeatures[0])}ListQuery, describe${pascal(pageFeatures[0])} } from '../../app/features/${pageFeatures[0]}';
import { ${pascal(pageFeatures[1])}Widget } from '../../app/features/${pageFeatures[1]}';
import { Interactive } from './Interactive';
import hero from './assets/hero.svg';
import badge from './assets/badge.svg';
${shape.photos ? "import photo from './assets/photo.png';\n" : ''}${mono ? "import '@fontsource/jetbrains-mono/400.css';\n" : ''}import './page.css';
${sectionImports}

const locales = {
${LOCALES.map(code => `  ${code}: () => import('i18n-iso-countries/langs/${code}.json'),`).join('\n')}
};
${table ? `
type Row = { id: string; label: string; value: number; updatedAt: string };
const column = createColumnHelper<Row>();
const columns = [column.accessor('label', { header: 'Name' }), column.accessor('value', { header: 'Value' })];

function ItemsTable({ rows }: { rows: Row[] }) {
  const table = useReactTable({ data: rows, columns, getCoreRowModel: getCoreRowModel() });
  return (
    <table>
      <thead>{table.getHeaderGroups().map(group => <tr key={group.id}>{group.headers.map(header => <th key={header.id}>{flexRender(header.column.columnDef.header, header.getContext())}</th>)}</tr>)}</thead>
      <tbody>{table.getRowModel().rows.map(row => <tr key={row.id}>{row.getVisibleCells().map(cell => <td key={cell.id}>{flexRender(cell.column.columnDef.cell, cell.getContext())}</td>)}</tr>)}</tbody>
    </table>
  );
}
` : ''}
export function Page() {
  const { data, loading } = useQuery(${pascal(pageFeatures[0])}ListQuery, { variables: { first: 20 } });
  const [country, setCountry] = useState('');
  useEffect(() => {
    let active = true;
    locales.${locale}().then(module => {
      countries.registerLocale(module.default ?? module);
      if (active) setCountry(countries.getName('CA', '${locale}') ?? '');
    });
    return () => { active = false; };
  }, []);
  const rows = (data as { items?: Array<{ id: string; label: string; value: number; updatedAt: string }> } | undefined)?.items ?? [];
  ${phone ? "const phone = parsePhoneNumberFromString('+14155550123')?.formatInternational() ?? '';" : "const phone = '';"}
  return (
    <div className="acme-page acme-page-${slug}" data-acme-page="${slug}">
      <header>
        <img src={hero} alt="" width="640" height="320" />
        <h1>${Name.replace(/([a-z])([A-Z0-9])/g, '$1 $2')}</h1>
        <img src={badge} alt="" width="160" height="80" />
${shape.photos ? '        <img src={photo} alt="" width="1200" height="800" data-acme="photo" />\n' : ''}${mono ? "        <code className=\"acme-mono\" style={{ fontFamily: \"'JetBrains Mono', monospace\" }}>const page = '" + slug + "';</code>\n" : ''}
        <p data-acme="country">{country}</p>
        <p data-acme="phone">{phone}</p>
      </header>
      <Interactive />
      {loading ? <p>Loading</p> : rows.length ? ${table ? '<ItemsTable rows={rows} />' : `<ul>{rows.map(row => <li key={row.id}>{describe${pascal(pageFeatures[0])}(row)}</li>)}</ul>`} : <p data-acme="empty">Nothing here</p>}
      <${pascal(pageFeatures[1])}Widget title="Related" />
${Array.from({ length: sections }, (_, section) => `      <${Name}Section${section} items={rows} />`).join('\n')}
    </div>
  );
}
`);
    const operation = `${pascal(pageFeatures[0])}ListQuery`;
    writer.write(`${dir}/index.workbench.tsx`, `import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'pages/${slug}',
  title: 'Acme/Pages/${Name}',
  adapter: 'react',
  source: { entry: './Page.tsx', export: 'Page' },
  requests: {
    'POST /graphql ${operation}': { body: { data: { items: ${JSON.stringify(records(rand, 12))} } } },
${shape.sentinels ? `    'GET /api/only-${slug}': { body: { owner: 'pages/${slug}' } },\n` : ''}  },
  states: {
    default: {},
    empty: { requests: { 'POST /graphql ${operation}': { body: { data: { items: [] } } } } },
  },
});
`);
    pages.push({ id: `pages/${slug}`, file: `${dir}/index.workbench.tsx`, sections, selector: `[data-acme-page="${slug}"]`, ...(mono ? { mono: true } : {}), edit: { component: `${dir}/Interactive.tsx`, module: `${dir}/sections/Section0.tsx` } });
  }
  return pages;
}

function generateBroken(writer) {
  writer.write('src/pages/broken-compile/Page.tsx', "import { Missing } from './does-not-exist';\n\nexport function Page() {\n  return <Missing />;\n}\n");
  writer.write('src/pages/broken-compile/index.workbench.tsx', "import { definePreview } from '@canonic2/workbench';\n\nexport default definePreview({\n  id: 'pages/broken-compile',\n  title: 'Acme/Broken/Compile',\n  adapter: 'react',\n  source: { entry: './Page.tsx', export: 'Page' },\n});\n");
  writer.write('src/pages/broken-mount/Page.tsx', "export function Page(): never {\n  throw new Error('Acme broken mount');\n}\n");
  writer.write('src/pages/broken-mount/index.workbench.tsx', "import { definePreview } from '@canonic2/workbench';\n\nexport default definePreview({\n  id: 'pages/broken-mount',\n  title: 'Acme/Broken/Mount',\n  adapter: 'react',\n  source: { entry: './Page.tsx', export: 'Page' },\n});\n");
  return [
    { id: 'pages/broken-compile', file: 'src/pages/broken-compile/index.workbench.tsx', broken: 'compile' },
    { id: 'pages/broken-mount', file: 'src/pages/broken-mount/index.workbench.tsx', broken: 'mount' },
  ];
}

// Japanese text spread over the CJK block, so Noto Sans JP loads many of its
// unicode-range files, and icon ligatures for the variable icon font.
const JAPANESE = Array.from({ length: 320 }, (_, index) => String.fromCodePoint(0x4E00 + ((index * 97) % 0x5000))).join('');
const ICONS = ['home', 'search', 'settings', 'person', 'calendar_month', 'receipt_long', 'construction', 'map'];

// The extended isolation sentinels (see formats.mjs): each page leaves a
// listener, an interval, a portal, a <body> class, a global and a request
// mock behind, and on entering records in <html data-acme-leaks> what an
// earlier page left. The probe runs after mount without delaying readiness.
function generateSentinels(writer, owners) {
  writer.write('src/workbench/sentinels.ts', `const OWNERS = ${JSON.stringify(owners)};
const key = (owner: string) => owner.replace(/[^a-z0-9]+(\\w)/g, (_, char: string) => char.toUpperCase());
const tickKey = (owner: string) => 'acmeTick' + key(owner).replace(/^\\w/, char => char.toUpperCase());

function leak(found: string) {
  const html = document.documentElement;
  const list = new Set((html.dataset.acmeLeaks || '').split(' ').filter(Boolean));
  list.add(found);
  html.dataset.acmeLeaks = Array.from(list).join(' ');
}

export function enter(owner: string) {
  const html = document.documentElement;
  if (html.dataset.acmeLeaks === undefined) html.dataset.acmeLeaks = '';
  const heard: string[] = [];
  window.dispatchEvent(new CustomEvent('acme-leak-probe', { detail: heard }));
  for (const other of heard) if (other !== owner) leak('listener:' + other);
  for (const node of Array.from(document.querySelectorAll('[data-acme-portal]'))) {
    const other = node.getAttribute('data-acme-portal');
    if (other !== owner) leak('portal:' + other);
  }
  for (const name of Array.from(document.body.classList)) {
    if (name.startsWith('acme-owner-') && name !== 'acme-owner-' + key(owner)) leak('class:' + name);
  }
  for (const other of OWNERS) {
    if (other !== owner && (window as unknown as Record<string, unknown>)['acmeOwner' + key(other)]) leak('global:' + other);
  }
  window.addEventListener('acme-leak-probe', event => { (event as CustomEvent<string[]>).detail.push(owner); });
  window.setInterval(() => { document.documentElement.dataset[tickKey(owner)] = '1'; }, 25);
  const portal = document.createElement('div');
  portal.dataset.acmePortal = owner;
  portal.hidden = true;
  document.body.append(portal);
  document.body.classList.add('acme-owner-' + key(owner));
  (window as unknown as Record<string, unknown>)['acmeOwner' + key(owner)] = true;
  setTimeout(() => { probe(owner).catch(() => {}); }, 0);
}

async function probe(owner: string) {
  const html = document.documentElement;
  for (const other of OWNERS) delete html.dataset[tickKey(other)];
  await new Promise(resolve => setTimeout(resolve, 80));
  for (const other of OWNERS) if (other !== owner && html.dataset[tickKey(other)]) leak('timer:' + other);
  let previous: string | null = null;
  try { previous = localStorage.getItem('acme-previous'); localStorage.setItem('acme-previous', owner); } catch (_) {}
  if (previous && previous !== owner) {
    try {
      const body = await (await fetch('/api/only-' + previous.replace(/^pages\\//, ''))).json();
      if (body && body.owner === previous) leak('request:' + previous);
    } catch (_) { /* Not mocked. */ }
  }
  html.dataset.acmeProbed = owner;
}
`);
}

const FONT_IMPORTS = {
  inter: "import '@fontsource/inter/400.css';\nimport '@fontsource/inter/600.css';\n",
  system: '',
  web: "import '@fontsource-variable/inter/standard.css';\nimport '@fontsource-variable/inter/standard-italic.css';\nimport '@fontsource/noto-sans-jp/400.css';\nimport '@fontsource-variable/material-symbols-outlined/full.css';\n",
};
const FONT_FAMILY = { inter: "'Inter', ", system: '', web: "'Inter Variable', " };

function generateEnvironment(writer, large, shape = {}) {
  // Sentinels: a module-level set of preview IDs rendered by this module
  // instance, and a count of live window listeners. A fresh document per
  // preview keeps both at 1; shared module state or leaked listeners raise them.
  writer.write('src/workbench/environment.tsx', `import type { ReactNode } from 'react';
${large ? `import { ApolloClient, HttpLink, InMemoryCache } from '@apollo/client';
import { ApolloProvider } from '@apollo/client/react';
${FONT_IMPORTS[shape.fonts || 'inter']}import { Shell } from '../app/Shell';
import { useSession } from './session';
` : ''}${shape.sentinels ? "import { enter } from './sentinels';\n" : ''}import '../styles/global.css';

const seenPreviews = new Set<string>();
let liveListeners = 0;

export function setup(context: { id: string }) {
${shape.sentinels ? '  enter(context.id);\n' : ''}  seenPreviews.add(context.id);
  document.documentElement.dataset.acmeSeenPreviews = String(seenPreviews.size);
  const listener = () => {};
  window.addEventListener('acme-ping', listener);
  liveListeners++;
  document.documentElement.dataset.acmeListeners = String(liveListeners);
  return () => {
    window.removeEventListener('acme-ping', listener);
    liveListeners--;
    document.documentElement.dataset.acmeListeners = String(liveListeners);
  };
}

${shape.fonts === 'web' ? `function WebFonts() {
  return (
    <div className="acme-web-fonts">
      <p className="acme-jp" lang="ja">${JAPANESE}</p>
      <p>${ICONS.map(icon => '<span className="acme-icon">' + icon + '</span>').join('')}</p>
    </div>
  );
}

` : ''}export function wrap(tree: ReactNode) {
${large ? `  const client = new ApolloClient({ cache: new InMemoryCache(), link: new HttpLink({ uri: '/graphql' }) });
  useSession.setState({ renders: useSession.getState().renders + 1 });
  return <ApolloProvider client={client}>${shape.fonts === 'web' ? '<WebFonts />' : ''}<Shell>{tree}</Shell></ApolloProvider>;` : '  return <div className="acme-root">{tree}</div>;'}
}
`);
  if (large) writer.write('src/workbench/session.ts', `import { create } from 'zustand';

export const useSession = create<{ renders: number }>(() => ({ renders: 0 }));
`);
  writer.write('src/styles/global.css', `:root { --acme-accent: #3355dd; }
body { margin: 0; font-family: ${FONT_FAMILY[large ? shape.fonts || 'inter' : 'system']}system-ui, sans-serif; color: #1d2433; }${shape.fonts === 'web' ? "\n.acme-jp { font-family: 'Noto Sans JP', sans-serif; font-size: 13px; margin: 4px 12px; }\n.acme-icon { font-family: 'Material Symbols Outlined Variable'; font-size: 24px; font-feature-settings: 'liga'; margin: 0 4px; }" : ''}
.acme-shell { display: grid; grid-template-columns: 220px 1fr; }
.acme-shell nav { display: flex; flex-direction: column; gap: 4px; padding: 12px; overflow: hidden; max-height: 100vh; }
`);
}

/**
 * Writes an Acme project into `root`, which must be empty or missing.
 * Returns a manifest describing what it generated.
 */
export function generate(root, { size = 'large', seed = 1 } = {}) {
  if (!SIZES[size]) throw new Error('Unknown fixture size: ' + size);
  if (fs.existsSync(root) && fs.readdirSync(root).length) throw new Error('Fixture directory is not empty: ' + root);
  const shape = SIZES[size];
  const rand = random(seed);
  const writer = new Writer(root);
  const large = size !== 'small';
  writer.write('package.json', JSON.stringify({ name: 'acme-lab-' + LOCKFILE[size], private: true, type: 'module', dependencies: DEPENDENCIES[size] }, null, 2) + '\n');
  writer.write('workbench.yaml', 'name: Acme lab\npreviews:\n  include:\n    - src/**/*.workbench.tsx\n');
  writer.write('workbench.config.ts', `import { defineConfig } from '@canonic2/workbench';
import { slowPlugin } from './workbench/slow-plugin';

export default defineConfig({
  environment: './src/workbench/environment.tsx',
  plugins: process.env.ACME_SLOW_PLUGIN === '1' ? [slowPlugin] : [],
});
`);
  // Loads every project source file itself, as some project plugins do, so
  // the compiler cannot reuse its own file cache for them.
  writer.write('workbench/slow-plugin.ts', `import { readFile } from 'node:fs/promises';

export const slowPlugin = {
  name: 'acme-slow',
  setup(build: { onLoad(options: { filter: RegExp }, callback: (args: { path: string }) => Promise<{ contents: string; loader: string }>): void }) {
    build.onLoad({ filter: /[\\\\/]src[\\\\/].*\\.tsx?$/ }, async args => ({
      contents: await readFile(args.path, 'utf8'),
      loader: args.path.endsWith('.tsx') ? 'tsx' : 'ts',
    }));
  },
};
`);
  generateEnvironment(writer, large, shape);
  const ui = generateUi(writer, rand, shape.components, shape.componentBytes, large);
  let previews = [];
  if (large) {
    const features = generateFeatures(writer, rand, shape.features, ui);
    previews = generatePages(writer, rand, shape.pages, ui, features, shape.sections, shape);
    if (shape.sentinels) generateSentinels(writer, previews.map(preview => preview.id));
    previews.push(...generateBroken(writer));
  } else {
    for (const name of ui) {
      writer.write(`src/ui/${name}.workbench.tsx`, `import { definePreview } from '@canonic2/workbench';

export default definePreview({
  id: 'components/${name.toLowerCase()}',
  title: 'Acme/Components/${name}',
  adapter: 'react',
  source: { entry: './${name}.tsx', export: '${name}' },
  inputs: { title: '${name}', tone: 'neutral' },
  states: { default: {}, accent: { inputs: { tone: 'accent' } } },
});
`);
      previews.push({ id: `components/${name.toLowerCase()}`, file: `src/ui/${name}.workbench.tsx`, selector: `[data-component="${name}"]`, edit: { component: `src/ui/${name}.tsx`, module: `src/ui/${name}.tsx` } });
    }
  }
  const manifest = { size, seed, fonts: large ? shape.fonts : 'system', photos: !!shape.photos, sentinels: !!shape.sentinels, files: writer.files, sourceBytes: writer.bytes, dependencies: DEPENDENCIES[size], previews };
  fs.writeFileSync(path.join(root, 'acme-manifest.json'), JSON.stringify(manifest, null, 2) + '\n');
  return manifest;
}
