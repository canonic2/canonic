import { dialog } from 'electron';
import type { BrowserWindow } from 'electron';
import type { Studio } from '../../src/application/studio.ts';
import type { Workspaces } from './workspaces.ts';
import type { CommandPayload } from './request.ts';
import { required, validatePayload } from './request.ts';

export function createCommands({
  studio,
  window,
  workspaces,
  publish,
}: {
  studio: Studio;
  window: BrowserWindow;
  workspaces: Workspaces;
  publish: () => void;
}) {
  const actions: Record<string, (payload: CommandPayload) => unknown> = {
    state: () => studio.snapshot(),
    logs: (payload) => studio.logs(required(payload, 'sessionId')),
    select: async (payload) => {
      const id = await studio.select(required(payload, 'projectId'), payload.sessionId);
      publish();
      await workspaces.show(id);
    },
    workspace: (payload) => workspaces.show(required(payload, 'sessionId'), payload.mode),
    newSession: async (payload) => {
      const session = await studio.createSession(
        required(payload, 'projectId'),
        required(payload, 'name'),
        payload.base || 'HEAD',
      );
      publish();
      await workspaces.show(session.id);
    },
    openProject: async () => {
      const result = await dialog.showOpenDialog(window, {
        title: 'Open a Git project',
        properties: ['openDirectory'],
      });
      if (result.canceled) return;
      const project = await studio.addProject(result.filePaths[0]);
      publish();
      await workspaces.show(project.activeSessionId);
    },
    closeProject: async (payload) => {
      const project = studio.projects.find(required(payload, 'projectId'));
      await studio.closeProject(project.id);
      for (const session of project.sessions) workspaces.close(session.id);
      publish();
      const state = studio.snapshot();
      const next = state.projects.find((item) => item.id === state.activeProjectId);
      if (next) await workspaces.show(next.activeSessionId);
    },
    startService: (payload) => studio.startService(required(payload, 'sessionId')),
    stopService: async (payload) => {
      const id = required(payload, 'sessionId');
      await studio.stopService(id);
      workspaces.close(id, 'preview');
    },
  };
  return async (action: string, payload: unknown = {}) => {
    if (!Object.hasOwn(actions, action)) throw new Error('Unknown Studio action.');
    const value = await actions[action](validatePayload(payload));
    publish();
    return ['state', 'logs'].includes(action) ? value : studio.snapshot();
  };
}
