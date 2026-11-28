// Preserve native paint scheduling before application scripts or virtual time.
(() => {
  const key = Symbol.for('canonic.testrun.native-timing.v1');
  if (!globalThis[key]) {
    Object.defineProperty(globalThis, key, {
      value: Object.freeze({ raf: requestAnimationFrame.bind(globalThis), Promise }),
    });
  }
})();
