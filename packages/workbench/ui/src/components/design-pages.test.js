/* Every Workbench element has its docs page in the Workbench space, as
   specs/ui/web-components.md#design-pages requires: the page, its examples, and
   the definition that lists it under Components. */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const components = fileURLToPath(new URL('.', import.meta.url));
const design = path.resolve(components, '../../../design');
const views = path.resolve(components, '../views');

/** Element folders under `folder`, as `{ name, at }` with `at` relative to `src/`. */
function elementsIn(folder, at) {
  if (!fs.existsSync(folder)) return [];
  return fs
    .readdirSync(folder, { withFileTypes: true })
    .filter((entry) => entry.isDirectory() && fs.existsSync(path.join(folder, entry.name, 'element.js')))
    .map((entry) => ({ name: entry.name, at: `${at}/${entry.name}/` }));
}

// Shared elements, and those a single view keeps in its own `components/`.
const elements = [
  ...elementsIn(components, 'src/components'),
  ...(fs.existsSync(views) ? fs.readdirSync(views, { withFileTypes: true }) : [])
    .filter((entry) => entry.isDirectory())
    .flatMap((entry) =>
      elementsIn(path.join(views, entry.name, 'components'), `src/views/${entry.name}/components`),
    ),
];

void test('every component has a design page listed under Components', () => {
  assert.ok(elements.length > 0);
  for (const { name, at } of elements) {
    const file = (ending) => path.join(design, name + ending);
    for (const ending of ['.md', '.examples.ts', '.workbench.ts']) {
      assert.ok(fs.existsSync(file(ending)), `design/${name}${ending} is missing for ${at}`);
    }
    const definition = fs.readFileSync(file('.workbench.ts'), 'utf8');
    assert.match(
      definition,
      new RegExp(`id: 'components/${name}'`),
      `design/${name}.workbench.ts lists it as components/${name}`,
    );
    assert.match(
      definition,
      /title: 'Components\/[^/']+\/[^/']+'/,
      `design/${name}.workbench.ts places it under Components, in its area's group`,
    );
    assert.match(
      definition,
      new RegExp(`examples: './${name}\\.examples\\.ts'`),
      `design/${name}.workbench.ts renders its examples`,
    );
    assert.match(
      fs.readFileSync(file('.md'), 'utf8'),
      /^## Public API$/m,
      `design/${name}.md documents its public API`,
    );
  }
});
