// Fixture-only observations. Never change the observed button or force a frame.
(selector) => {
  const button = document.querySelector(selector);
  const output = document.createElement('input');
  output.id = 'Acme-readiness-observation';
  output.hidden = true;
  output.readOnly = true;
  document.body.append(output);
  let frames = 0, lastFrameAt = null, changes = 0;
  const save = () => {
    output.value = JSON.stringify({frames, lastFrameAt, changes,
      sampledAt: performance.now(), visibility: document.visibilityState,
      focused: document.hasFocus(), hidden: button.hidden, disabled: button.disabled});
  };
  new MutationObserver(() => { ++changes; save(); })
    .observe(button, {attributes: true, attributeFilter: ['hidden', 'disabled']});
  document.addEventListener('visibilitychange', save);
  window.addEventListener('focus', save);
  window.addEventListener('blur', save);
  const frame = at => { ++frames; lastFrameAt = at; save(); requestAnimationFrame(frame); };
  save();
  requestAnimationFrame(frame);
}
