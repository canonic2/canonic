import { freePort, launch, stopProcess, waitUntil, waitForExit } from '../../platform/processes.ts';
import { compose, prepareCompose } from './compose.ts';
import type { RuntimeTask, RuntimeFailure } from './types.ts';
import { asError, errorMessage } from '../../platform/errors.ts';
import type { RuntimeRecipe } from '../configuration/types.ts';
import type { Session, Project } from '../state/types.ts';

export async function stopRuntime(task?: RuntimeTask) {
  if (!task) return;
  // Try both cleanups even if one fails; retain the task so cleanup can be retried.
  const results = await Promise.allSettled([
    stopProcess(task.process),
    task.composeStarted ? compose(task, ['down', '--timeout', '3']) : Promise.resolve(),
  ]);
  const failures = results.filter((result) => result.status === 'rejected');
  if (failures.length)
    throw new AggregateError(
      failures.map((result) => asError(result.reason)),
      'Session cleanup failed',
    );
  task.composeStarted = false;
}

export async function startRuntime({
  recipe,
  session,
  project,
  env,
}: {
  recipe: RuntimeRecipe;
  session: Session;
  project: Project;
  env: NodeJS.ProcessEnv;
}): Promise<RuntimeTask> {
  const names = recipe.ports || ['PORT'];
  const ports: Record<string, number> = {};
  for (const name of names) {
    let port;
    do {
      port = await freePort();
    } while (Object.values(ports).includes(port));
    ports[name] = port;
  }
  const previewPort = recipe.previewPort || names[0];
  if (!ports[previewPort]) throw new Error('previewPort must name an allocated port.');
  const task: RuntimeTask = {
    checkout: session.checkout,
    ports,
    port: ports[previewPort],
    url: `http://127.0.0.1:${ports[previewPort]}`,
    env: {
      ...env,
      ...recipe.env,
      ...Object.fromEntries(Object.entries(ports).map(([name, port]) => [name, String(port)])),
      STUDIO_SESSION_ID: session.id,
      STUDIO_CHECKOUT: session.checkout,
      ELECTRON_RUN_AS_NODE: '1',
    },
    composeName: `studio_${project.id}_${session.id}`,
    composeOutput: '',
    composeStarted: false,
    process: null,
  };
  const provision = async (command: string[]) => {
    const executable =
      command[0] === '$NODE' ? process.env.STUDIO_NODE || process.execPath : command[0];
    // Own provision descendants too: a timeout must not leave installers/servers running.
    task.process = launch(executable, command.slice(1), { cwd: session.checkout, env: task.env });
    await waitForExit(task.process, recipe.provisionTimeoutMs);
    await stopProcess(task.process);
    task.process = null;
  };
  try {
    if (recipe.provision) await provision(recipe.provision);
    await prepareCompose(task, recipe);
    const executable =
      recipe.command[0] === '$NODE'
        ? process.env.STUDIO_NODE || process.execPath
        : recipe.command[0];
    task.process = launch(executable, recipe.command.slice(1), {
      cwd: session.checkout,
      env: task.env,
    });
    await waitUntil(
      task.process,
      async () => {
        try {
          const response = await fetch(`${task.url}${recipe.readyPath || '/health'}`, {
            signal: AbortSignal.timeout(800),
          });
          const body: unknown = await response.json();
          return (
            response.ok &&
            body !== null &&
            typeof body === 'object' &&
            'sessionId' in body &&
            body.sessionId === session.id
          );
        } catch {
          return false;
        }
      },
      recipe.timeoutMs,
    );
    return task;
  } catch (error) {
    const failure: RuntimeFailure = asError(error);
    try {
      await stopRuntime(task);
    } catch (cleanupError) {
      failure.cleanupTask = task;
      failure.message += `; ${errorMessage(cleanupError)}`;
    }
    throw failure;
  }
}
