/** Native modal lifetime; the consumer decides whether a close request is allowed. */
export function bindModal({ dialog, cancel, requestClose, signal, backdrop = true }) {
  const dismiss = (event) => {
    event.preventDefault();
    requestClose(event);
  };
  dialog.addEventListener('cancel', dismiss, { signal });
  cancel.addEventListener('click', dismiss, { signal });
  if (backdrop)
    dialog.addEventListener(
      'click',
      (event) => {
        if (event.target === dialog) dismiss(event);
      },
      { signal },
    );
  const close = () => {
    if (dialog.open) dialog.close();
  };
  if (signal.aborted) close();
  else signal.addEventListener('abort', close, { once: true });
}

/** Sync visibility without re-opening a modal or disturbing its active field. */
export function syncModal(dialog, { open, connected, focus }) {
  if (open && !dialog.open && connected) {
    dialog.showModal();
    focus?.();
  } else if (!open && dialog.open) dialog.close();
}
