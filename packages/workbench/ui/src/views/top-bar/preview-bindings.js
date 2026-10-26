/** The controls' capability owns inputs, actions, selection and open state. */
export function bindPreviewControls({ controls, button, panel, signal, report }) {
  if (signal.aborted) return;
  const failed = new AbortController();
  const own = AbortSignal.any([signal, failed.signal]);
  function present(error) {
    if (own.aborted) return globalThis.reportError(error);
    try {
      report(error);
    } catch (reportFailure) {
      globalThis.reportError(reportFailure);
    }
  }
  function act(run) {
    try {
      Promise.resolve(run()).catch(present);
    } catch (error) {
      present(error);
    }
  }
  try {
    controls.state.watch(
      controls.select.model,
      (value) => {
        panel.model = value;
      },
      { signal: own },
    );
    controls.state.watch(
      controls.select.available,
      (value) => {
        button.hidden = !value;
      },
      { signal: own },
    );
    controls.state.watch(
      controls.select.opened,
      (value) => {
        panel.hidden = !value;
        button.setAttribute('aria-expanded', String(value));
      },
      { signal: own },
    );
    button.addEventListener('click', () => act(() => controls.toggle()), { signal: own });
    panel.addEventListener(
      'wb-preview-input',
      (event) => act(() => controls.changeInput(event.detail.name, event.detail.value)),
      { signal: own },
    );
    panel.addEventListener('wb-preview-reset', () => act(() => controls.resetScenario()), {
      signal: own,
    });
  } catch (error) {
    failed.abort();
    throw error;
  }
}
