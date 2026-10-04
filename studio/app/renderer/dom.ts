interface Elements {
  projects: HTMLElement;
  sessions: HTMLElement;
  'service-status': HTMLElement;
  endpoint: HTMLElement;
  'toggle-service': HTMLButtonElement;
  'project-name': HTMLElement;
  'session-name': HTMLElement;
  branch: HTMLElement;
  status: HTMLElement;
  checkout: HTMLElement;
  'new-session': HTMLButtonElement;
  'open-workspace': HTMLButtonElement;
  'open-project': HTMLButtonElement;
  welcome: HTMLElement;
  loading: HTMLElement;
  logs: HTMLElement;
  'session-form': HTMLFormElement;
  error: HTMLElement;
  'task-name': HTMLInputElement;
  'base-ref': HTMLInputElement;
  'cancel-session': HTMLButtonElement;
}
export function element<K extends keyof Elements>(id: K): Elements[K] {
  const value = document.getElementById(id);
  if (!value) throw new Error(`Missing Studio control: ${id}`);
  return value as Elements[K];
}
