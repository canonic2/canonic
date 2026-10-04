import path from 'node:path';
import type { ComposeModel, RuntimeTask } from './types.ts';

export function validateCompose(config: ComposeModel, task: RuntimeTask) {
  const { checkout, ports } = task;
  for (const resources of [config.volumes, config.networks]) {
    for (const resource of Object.values(resources || {})) {
      if (resource.external || !resource.name?.startsWith(task.composeName + '_'))
        throw new Error('Compose resources must be scoped to the session project.');
    }
  }
  for (const service of Object.values(config.services || {})) {
    if (service.container_name || service.network_mode || service.privileged)
      throw new Error('Compose service bypasses session isolation.');
    for (const port of service.ports || []) {
      if (port.host_ip !== '127.0.0.1' || !Object.values(ports).includes(Number(port.published)))
        throw new Error('Compose ports must use allocated loopback ports.');
    }
    for (const volume of service.volumes || []) {
      if (volume.type === 'bind' && !path.resolve(volume.source).startsWith(checkout + path.sep))
        throw new Error('Compose bind mounts must stay inside the checkout.');
    }
  }
}
