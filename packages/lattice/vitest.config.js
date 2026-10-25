/** @file Configures isolated Node and real Chromium test projects for Lattice. */
import { defineConfig } from 'vitest/config';
import { playwright } from '@vitest/browser-playwright';

export default defineConfig({
  test: {
    projects: [
      {
        test: {
          name: 'unit',
          environment: 'node',
          include: ['src/**/*.test.js', 'tests/**/*.test.js', 'examples/**/*.test.js'],
          exclude: ['**/*.browser.test.js'],
          maxWorkers: 2,
          // Retention tests force garbage collection to check what is released.
          execArgv: ['--expose-gc'],
        },
      },
      {
        test: {
          name: 'browser',
          include: [
            'src/**/*.browser.test.js',
            'tests/**/*.browser.test.js',
            'examples/**/*.browser.test.js',
          ],
          maxWorkers: 1,
          browser: {
            enabled: true,
            headless: true,
            provider: playwright(),
            instances: [{ browser: 'chromium' }],
          },
        },
      },
    ],
  },
});
