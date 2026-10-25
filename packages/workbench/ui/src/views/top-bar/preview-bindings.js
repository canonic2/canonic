/** The controls' capability owns inputs, actions, selection and open state. */
export function bindPreviewControls({ controls, button, panel, signal, report }) {
  controls.state.watch(
    controls.select.model,
    (value) => {
      panel.model = value;
    },
    { signal },
  );
  controls.state.watch(
    controls.select.available,
    (value) => {
      button.hidden = !value;
    },
    { signal },
  );
  controls.state.watch(
    controls.select.opened,
    (value) => {
      panel.hidden = !value;
      button.setAttribute('aria-expanded', String(value));
    },
    { signal },
  );
  function act(run) {
    try {
      Promise.resolve(run()).catch(report);
    } catch (error) {
      report(error);
    }
  }
  button.addEventListener('click', () => controls.toggle(), { signal });
  panel.addEventListener(
    'wb-preview-input',
    (event) => act(() => controls.changeInput(event.detail.name, event.detail.value)),
    { signal },
  );
  panel.addEventListener('wb-preview-reset', () => act(() => controls.resetScenario()), { signal });
}
