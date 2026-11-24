/* Copies text and confirms on the button for a moment. Does nothing where the
   clipboard API is unavailable. */
const timers = new WeakMap<HTMLElement, number>();

export function copyText(button: HTMLElement, text: string) {
  if (!navigator.clipboard) return;
  navigator.clipboard.writeText(text).then(() => {
    button.textContent = button.dataset.copied || 'Copied';
    clearTimeout(timers.get(button));
    timers.set(button, window.setTimeout(() => { button.textContent = button.dataset.copy || 'Copy'; }, 1600));
  }, () => {});
}
