import { fileURLToPath } from 'node:url';
import path from 'node:path';
import type { Session } from '../modules/state/types.ts';

export const executionRoot = fileURLToPath(new URL('../../', import.meta.url));
export const packageRoot =
  path.basename(executionRoot) === 'dist' ? path.dirname(executionRoot) : executionRoot;
export const defaultRuntime = path.join(packageRoot, '.runtime/code-server/bin/code-server');

export function sessionEnvironment(root: string, session: Session): NodeJS.ProcessEnv {
  const home = path.join(root, 'sessions', session.id);
  return {
    ...process.env,
    STUDIO_SESSION_ID: session.id,
    STUDIO_CHECKOUT: session.checkout,
    XDG_CONFIG_HOME: path.join(home, 'config'),
    XDG_DATA_HOME: path.join(home, 'data'),
    XDG_CACHE_HOME: path.join(home, 'cache'),
  };
}
