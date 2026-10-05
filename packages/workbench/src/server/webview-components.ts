import { createRequire } from 'node:module';
import { fileURLToPath } from 'node:url';

const require = createRequire(import.meta.url);

/** Direct webviews cannot execute .ts resources. Bundle the same source in
 * memory using the packaged compiler; no generated implementation is kept. */
export function webviewComponents(): string {
  const engine = require('../../preview/engine.cjs') as typeof import('esbuild');
  const result = engine.buildSync({
    entryPoints: [fileURLToPath(new URL('../components/bootstrap.ts', import.meta.url))],
    bundle: true, write: false, format: 'iife', platform: 'browser', target: 'es2022',
  });
  return result.outputFiles![0]!.text.replace(/<\/script/gi, '<\\/script');
}
