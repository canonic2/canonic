/* Every Workbench element has its docs page in the Workbench space, as
   specs/web-components.md#design-pages requires: the page, its examples, and
   the definition that lists it under Components. */
import { test } from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const components = fileURLToPath(new URL('.', import.meta.url));
const design = path.resolve(components, '../../design');

const elements = fs.readdirSync(components, { withFileTypes: true })
  .filter(entry => entry.isDirectory() && fs.existsSync(path.join(components, entry.name, 'element.ts')))
  .map(entry => entry.name);

test('every component has a design page listed under Components', () => {
  assert.ok(elements.length > 0);
  for (const name of elements) {
    const file = (ending: string) => path.join(design, name + ending);
    for (const ending of ['.md', '.examples.ts', '.workbench.ts']) {
      assert.ok(fs.existsSync(file(ending)), `design/${name}${ending} is missing for src/components/${name}/`);
    }
    const definition = fs.readFileSync(file('.workbench.ts'), 'utf8');
    assert.match(definition, new RegExp(`id: 'components/${name}'`), `design/${name}.workbench.ts lists it as components/${name}`);
    assert.match(definition, /title: 'Components\/[^/']+\/[^/']+'/, `design/${name}.workbench.ts places it under Components, in its area's group`);
    assert.match(definition, new RegExp(`examples: './${name}\\.examples\\.ts'`), `design/${name}.workbench.ts renders its examples`);
    assert.match(fs.readFileSync(file('.md'), 'utf8'), /^## Public API$/m, `design/${name}.md documents its public API`);
  }
});
