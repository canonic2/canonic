/* A reference to the current screen, without capturing or handing it off.
   Read the resolved view at click time: the address may still name a story
   that fell back to another, or a lens this screen doesn't have. */
(function () {
  function named(label, id) {
    return label + ' — `' + id + '`';
  }

  function text(view) {
    if (!view || !view.src || !view.url) return null;
    var item = view.item;
    var lens = view.lens;
    if (lens && lens.kind === 'storybook' && !view.story) return null;
    var lines = ['- Screen: ' + named(item.label, view.src)];
    if (view.story) {
      lines.push('- Story: ' + named(view.story.name, view.story.id));
    } else {
      var ref = lens && item.implementations && item.implementations[lens.key];
      var mapped = !lens || (ref && ref.states);
      var states = mapped ? item.states || [] : [];
      var state = states.find(function (entry) { return entry.id === view.state; });
      if (!state) state = states[0] || { id: 'default', label: 'Default' };
      lines.push('- State: ' + named(state.label, state.id));
    }
    lines.push('- Lens: ' + (lens ? named(lens.label, lens.key) + ' — `' + view.url + '`' : 'Design'));
    return lines.join('\n');
  }

  /* The editor embeds the workbench across origins, where the Clipboard API
     may be unavailable. Keep the same selection-based fallback as copy path. */
  function fallback(value) {
    var previous = document.activeElement;
    var input = document.createElement('textarea');
    input.value = value;
    input.setAttribute('readonly', '');
    input.style.position = 'fixed';
    input.style.opacity = '0';
    document.body.appendChild(input);
    try {
      input.select();
      if (!document.execCommand('copy')) throw new Error('Clipboard unavailable');
    } finally {
      input.remove();
      if (previous && previous.focus) previous.focus({ preventScroll: true });
    }
  }

  function copy(value) {
    if (navigator.clipboard && navigator.clipboard.writeText) {
      return navigator.clipboard.writeText(value).catch(function () { fallback(value); });
    }
    return Promise.resolve().then(function () { fallback(value); });
  }

  function say(message) {
    window.dispatchEvent(new CustomEvent('wb-say', { detail: { message: message } }));
  }

  document.getElementById('copyReference').addEventListener('click', function () {
    var view = window.wbView();
    var value = text(view);
    if (!value) {
      say(view ? 'Wait for the screen reference to load.' : 'Pick a page or a component first.');
      return;
    }
    copy(value).then(function () {
      say('Copied reference');
    }, function () {
      say('Couldn’t copy the reference.');
    });
  });

  window.wbReference = { text: text };
})();
