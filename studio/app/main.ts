import { app } from 'electron';
import path from 'node:path';
import { createDesktop } from './desktop/bootstrap.ts';
import { Studio } from '../src/application/studio.ts';
import { packageRoot, executionRoot } from '../src/platform/paths.ts';

const data = path.resolve(process.env.STUDIO_DATA_DIR || path.join(packageRoot, '.studio/app'));
app.setName('Canonic Studio');
app.setPath('userData', path.join(data, 'desktop'));
app.setPath('sessionData', app.getPath('userData'));
app.setPath('logs', path.join(data, 'logs'));
if (!app.requestSingleInstanceLock()) app.quit();
else
  app
    .whenReady()
    .then(async () => {
      const manager = await new Studio({
        root: data,
        runtime: process.env.STUDIO_IDE_RUNTIME,
      }).init();
      await createDesktop({ manager, root: executionRoot });
    })
    .catch((error) => {
      console.error(error);
      process.exitCode = 1;
      app.quit();
    });
