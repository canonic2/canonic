/** @file Checks that the package entry loads in Node without browser globals. */
import { expect, test } from 'vitest';

test('imports the package through its public ESM export in Node', async () => {
  await expect(import('canonic-lattice')).resolves.toBeDefined();
});
