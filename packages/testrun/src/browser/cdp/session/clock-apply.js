async (key, sequence, method, parameter) => {
  const state = globalThis[key];
  if (!state) throw Error('CLOCK_NOT_INSTALLED');
  if (state.sequence === sequence) return true;
  if (state.sequence !== sequence - 1) throw Error('CLOCK_PENDING');
  // Fence before any callback can run, including a callback that throws.
  state.sequence = sequence;
  await state.controller[method](parameter);
  return true;
}
