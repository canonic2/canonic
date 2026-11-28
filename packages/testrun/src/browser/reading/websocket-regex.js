// Execute a host-compiled ASCII DFA. Each URL byte takes at most one transition.
// A URLPattern component program runs over one parsed URL component instead.
((program, component = false) => {
  const bytes = value => Uint8Array.from(atob(value), byte => byte.charCodeAt(0));
  const classes = bytes(program.classes), transitions = bytes(program.transitions), acceptance = bytes(program.acceptance);
  const width = program.width, states = acceptance.length;
  if (classes.length !== 128 || !Number.isInteger(width) || width < 1 || width > 128
      || states < 1 || states > 512 || transitions.length !== states * width * 2
      || classes.length + transitions.length + states > 16384
      || classes.some(value => value >= width) || acceptance.some(value => value > 3))
    throw Error('WEBSOCKET_REGEX_INVALID');
  for (let offset = 0; offset < transitions.length; offset += 2) {
    if ((transitions[offset] | transitions[offset + 1] << 8) >= states)
      throw Error('WEBSOCKET_REGEX_INVALID');
  }
  return url => {
    if (!component && !url.startsWith('ws://') && !url.startsWith('wss://')) return false;
    if (url.length > 8192) throw new RangeError('WEBSOCKET_URL_BOUNDS');
    let state = 0, matched = !!(acceptance[0] & 1);
    for (let index = 0; index < url.length; index++) {
      const byte = url.charCodeAt(index);
      if (byte > 127) return false;
      if (!matched) {
        const offset = (state * width + classes[byte]) * 2;
        state = transitions[offset] | transitions[offset + 1] << 8;
        matched = !!(acceptance[state] & 1);
      }
    }
    return matched || !!(acceptance[state] & 2);
  };
})
