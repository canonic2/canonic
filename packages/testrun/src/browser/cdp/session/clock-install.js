(key, history, library) => {
  if (globalThis[key]) return;
  const { clock: controller } = library().install()(globalThis, {
    browserName: 'chromium',
    toFake: ['Date', 'setTimeout', 'clearTimeout', 'setInterval', 'clearInterval',
      'requestAnimationFrame', 'cancelAnimationFrame', 'requestIdleCallback',
      'cancelIdleCallback', 'performance'],
  });
  for (const entry of history) controller.log(...entry);
  // Replay before the first page script reads performance.timeOrigin, and
  // never schedule a native wakeup for a reconstructed paused clock.
  controller.now();
  if (!history.length) controller.resume();
  globalThis[key] = { controller, sequence: history.length };
}
