import { invoke } from './renderer/client.ts';
import {
  activeProject as selectProject,
  activeSession as selectSession,
} from './renderer/selectors.ts';
import { render as renderUI } from './renderer/render.ts';
import { element } from './renderer/dom.ts';
import type { StudioSnapshot } from '../src/application/types.ts';
import type { WorkspaceMode } from './contracts.ts';
import { errorMessage } from '../src/platform/errors.ts';

let state: StudioSnapshot | undefined;
let mode: WorkspaceMode = 'code';
let opening = false;
let formOpen = false;
let hasWorkspace = false;
const $ = element;
const activeProject = () => selectProject(state);
const activeSession = () => selectSession(state);

function report(error: unknown) {
  $('error').textContent = errorMessage(error);
  $('error').hidden = false;
  $('status').textContent = 'Action failed';
}

function render() {
  renderUI({
    state,
    mode,
    opening,
    select: (projectId, sessionId) => {
      void select(projectId, sessionId);
    },
    close: (projectId) => {
      void close(projectId);
    },
  });
}

async function close(projectId: string) {
  if (opening) return;
  opening = true;
  stage('loading');
  render();
  try {
    state = await invoke('closeProject', { projectId });
    mode = 'code';
    hasWorkspace = Boolean(activeSession());
    stage(hasWorkspace ? null : 'welcome');
  } catch (error) {
    stage(null);
    report(error);
  } finally {
    opening = false;
    render();
  }
}

function stage(kind: 'welcome' | 'loading' | 'logs' | 'session-form' | 'error' | null) {
  for (const id of ['welcome', 'loading', 'logs', 'session-form', 'error'] as const)
    $(id).hidden = id !== kind;
}

async function select(projectId: string, sessionId?: string) {
  if (opening) return;
  opening = true;
  mode = 'code';
  formOpen = false;
  stage('loading');
  render();
  try {
    state = await invoke('select', { projectId, sessionId });
    hasWorkspace = true;
    stage(null);
  } catch (error) {
    stage(null);
    report(error);
  } finally {
    opening = false;
    render();
  }
}

async function workspace(nextMode: WorkspaceMode) {
  if (opening || !activeSession()) return;
  formOpen = false;
  mode = nextMode;
  opening = true;
  stage(nextMode === 'logs' ? 'logs' : 'loading');
  render();
  try {
    state = await invoke('workspace', { sessionId: activeSession()!.id, mode });
    if (mode === 'logs')
      $('logs').textContent = await invoke('logs', { sessionId: activeSession()!.id });
    else {
      hasWorkspace = true;
      stage(null);
    }
  } catch (error) {
    stage(null);
    report(error);
  } finally {
    opening = false;
    render();
  }
}

$('open-workspace').onclick = () => workspace('code');
for (const button of document.querySelectorAll<HTMLButtonElement>('[data-mode]'))
  button.onclick = () => workspace(button.dataset.mode as WorkspaceMode);
$('new-session').onclick = async () => {
  if (opening || !activeSession()) return;
  try {
    await invoke('workspace', { sessionId: activeSession()!.id, mode: 'blank' });
    formOpen = true;
    stage('session-form');
    $('task-name').focus();
  } catch (error) {
    report(error);
  }
};
$('cancel-session').onclick = () => workspace('code');
$('session-form').onsubmit = async (event) => {
  event.preventDefault();
  if (opening) return;
  opening = true;
  const payload = {
    projectId: activeProject()!.id,
    name: $('task-name').value,
    base: $('base-ref').value,
  };
  stage('loading');
  render();
  try {
    state = await invoke('newSession', payload);
    formOpen = false;
    mode = 'code';
    hasWorkspace = true;
    $('task-name').value = '';
    stage(null);
  } catch (error) {
    stage('session-form');
    report(error);
  } finally {
    opening = false;
    render();
  }
};
$('open-project').onclick = async () => {
  if (opening) return;
  opening = true;
  try {
    state = await invoke('openProject');
    mode = 'code';
    formOpen = false;
    hasWorkspace = activeSession()?.ideStatus === 'ready';
    stage(hasWorkspace ? null : 'welcome');
  } catch (error) {
    report(error);
  } finally {
    opening = false;
    render();
  }
};
$('toggle-service').onclick = async () => {
  const session = activeSession();
  if (!session || opening) return;
  const stopping = session.serviceStatus === 'running';
  $('toggle-service').disabled = true;
  try {
    state = await invoke(stopping ? 'stopService' : 'startService', { sessionId: session.id });
    if (stopping && mode === 'preview') await workspace('code');
  } catch (error) {
    report(error);
  } finally {
    $('toggle-service').disabled = false;
    render();
  }
};
window.studio.onState((next) => {
  state = next;
  render();
});
invoke('state')
  .then((next) => {
    state = next;
    render();
  })
  .catch(report);
async function poll() {
  if (opening || formOpen) return;
  try {
    state = await invoke('state');
    render();
    if (mode === 'logs' && hasWorkspace && activeSession())
      $('logs').textContent = await invoke('logs', { sessionId: activeSession()!.id });
  } catch {
    /* The next poll retries transient IPC failures. */
  }
}
setInterval(() => {
  void poll();
}, 2000);
