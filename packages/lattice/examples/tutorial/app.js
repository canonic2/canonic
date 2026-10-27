/** @file Composes the tutorial's components and retained list with an explicit lifetime. */
import { TaskToggle, defineTaskToggle } from './task-toggle.js';
import { TaskLabel, defineTaskLabel } from './task-label.js';
import { createTasks } from './tasks.js';
import { mountTaskRows } from './task-list.js';

/**
 * Mounts the tutorial into an empty owned container; setup failure removes acquired resources.
 * @param {HTMLElement} container The view's container.
 * @param {{signal: AbortSignal}} options The view lifetime.
 * @returns {import('./tasks.js').Tasks | undefined} Actions, or undefined if already aborted.
 */
export function mountTutorial(container, { signal }) {
  if (signal.aborted) return;
  defineTaskToggle();
  defineTaskLabel();
  const failed = new AbortController();
  const own = AbortSignal.any([signal, failed.signal]);
  const tasks = createTasks();
  const toggle = new TaskToggle();
  const status = new TaskLabel();
  const list = document.createElement('ul');
  try {
    tasks.items.watchItem(
      'guide',
      tasks.select.done,
      (done) => {
        toggle.done = done;
        status.text = done ? 'Complete' : 'In progress';
      },
      { signal: own },
    );
    toggle.addEventListener(
      'acme-task-toggle',
      (event) => {
        const intent = /** @type {CustomEvent<{done: boolean}>} */ (event);
        try {
          tasks.setDone('guide', intent.detail.done);
        } catch (error) {
          reportError(error);
        }
      },
      { signal: own },
    );
    mountTaskRows(list, tasks, { signal: own });
    own.addEventListener(
      'abort',
      () => {
        for (const node of [list, status, toggle]) {
          try {
            node.remove();
          } catch (error) {
            reportError(error);
          }
        }
      },
      { once: true },
    );
    container.append(toggle, status, list);
  } catch (error) {
    failed.abort();
    throw error;
  }
  return tasks;
}
