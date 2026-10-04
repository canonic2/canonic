import { app } from 'electron';
import path from 'node:path';
import assert from 'node:assert/strict';
import { createDesktop } from '../../app/desktop/bootstrap.ts';
import { Studio } from '../../src/application/studio.ts';
import { packageRoot, executionRoot } from '../../src/platform/paths.ts';
import { runFullstack } from './fullstack.ts';
import { fullstackBrowser } from './fullstack-browser.ts';
import { seedDemo } from '../helpers/projects.ts';
import { runSmoke } from './ide-smoke.ts';
import * as fs from 'node:fs/promises';
const root = packageRoot;
const fullstack = process.argv.includes('--fullstack');
app.setPath('userData', path.join(root, '.studio/test-desktop'));
app.setPath('sessionData', app.getPath('userData'));
app.setPath('logs', path.join(root, '.studio/test-logs'));
if (!app.requestSingleInstanceLock()) app.exit(1);
else
  app
    .whenReady()
    .then(async () => {
      const manager = await new Studio({
        root: path.join(root, '.studio/tests', String(Date.now())),
      }).init();
      const context = await createDesktop({ manager, root: executionRoot });
      try {
        if (process.argv.includes('--empty')) {
          assert.equal(manager.snapshot().projects.length, 0);
          assert.equal(manager.runtimes.records.size, 0);
          assert.equal(manager.ide.records.size, 0);
          await new Promise((resolve) => setTimeout(resolve, 300));
          assert.equal(
            await context.window.webContents.executeJavaScript(
              `document.querySelector('#toggle-service').disabled && document.querySelector('#new-session').disabled`,
            ),
            true,
          );
          assert.equal(
            await context.window.webContents.executeJavaScript(
              `document.querySelector('#projects').children.length`,
            ),
            0,
          );
          console.log('Clean startup: no demo projects, processes or IDE servers');
        } else if (fullstack) {
          await runFullstack({
            manager,
            browser: (fixture) => fullstackBrowser({ ...context, ...fixture }),
          });
        } else {
          await seedDemo(manager);
          for (const project of manager.snapshot().projects)
            for (const session of project.sessions) await manager.startService(session.id);
          await runSmoke({ ...context, root });
        }
        console.log('STUDIO_SMOKE_PASSED');
      } catch (error) {
        console.error('STUDIO_SMOKE_FAILED', error);
        await fs.mkdir(path.join(root, 'artifacts'), { recursive: true });
        for (const [key, view] of context.views) {
          try {
            await fs.writeFile(
              path.join(root, 'artifacts', `failure-${key.replace(':', '-')}.txt`),
              await view.webContents.executeJavaScript('document.body.innerText'),
            );
          } catch {
            /* A destroyed workspace has no diagnostic text. */
          }
        }
        process.exitCode = 1;
      }
      app.quit();
    })
    .catch((error) => {
      console.error(error);
      app.exit(1);
    });
