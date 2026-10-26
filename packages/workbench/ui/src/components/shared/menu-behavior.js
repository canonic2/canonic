/** Opt-in menu mechanics. Components supply their eligible and preferred items. */
export function createMenuBehavior({
  host,
  root,
  trigger,
  menu,
  disabled,
  items,
  preferred,
  toggled,
}) {
  let open = false;
  function setOpen(value, user = false, focus = false) {
    const next = !!value && !disabled();
    const changed = open !== next;
    open = next;
    menu.hidden = !next;
    trigger.setAttribute('aria-expanded', String(next));
    if (focus) {
      if (next) (preferred() || items()[0])?.focus();
      else trigger.focus();
    }
    if (changed && user) toggled(next);
  }
  return {
    get open() {
      return open;
    },
    setOpen,
    connect(signal) {
      root.addEventListener(
        'keydown',
        (event) => {
          if (disabled()) return;
          if (event.key === 'Escape' && open) {
            event.preventDefault();
            event.stopPropagation();
            setOpen(false, true, true);
            return;
          }
          // Restore the sequential focus starting point, then let native Tab run.
          if (event.key === 'Tab' && open) {
            setOpen(false, true, true);
            return;
          }
          if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return;
          if (root.activeElement === trigger) {
            if (!['ArrowDown', 'ArrowUp'].includes(event.key)) return;
            event.preventDefault();
            setOpen(true, true, true);
            return;
          }
          const eligible = items();
          if (!open || !eligible.length || !menu.contains(root.activeElement)) return;
          event.preventDefault();
          const at = eligible.indexOf(root.activeElement);
          const next =
            event.key === 'Home'
              ? 0
              : event.key === 'End'
                ? eligible.length - 1
                : (at + (event.key === 'ArrowDown' ? 1 : -1) + eligible.length) % eligible.length;
          eligible[next]?.focus();
        },
        { signal },
      );
      host.ownerDocument.addEventListener(
        'pointerdown',
        (event) => {
          if (open && !event.composedPath().includes(host)) setOpen(false, true);
        },
        { signal },
      );
      host.ownerDocument.defaultView?.addEventListener(
        'blur',
        () => {
          if (open) setOpen(false, true);
        },
        { signal },
      );
    },
  };
}
