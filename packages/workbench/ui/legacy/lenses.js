/* Where a lens points
   -------------------
   The address the frame loads for a page seen through an implementation.
   Pure string work, kept apart from the shell so it can be checked without a
   browser.

   url         the implementation's base plus the page's path there — the
               path for the current state when the page mapped one, else the
               one path it gave.
   storyUrl    a single story, alone: Storybook's iframe.html with the story's
               id and viewMode=story shows the story and none of Storybook's
               own chrome.
   storyOpenUrl the same story inside Storybook proper, for "open on its own".
   selectStory asks a loaded preview to switch stories without navigation.
   storybookEvent reads acknowledgements from that preview's channel.
   pick        which of a title's stories the address means: the one whose
               state matches, else the first.
   upstream    an address at the implementation's own origin rather than its
               proxy, for what people and agents read. */
(function () {
  function url(impl, ref, state) {
    var path = (state && ref.states && ref.states[state]) || ref.path;
    return (impl.base || '') + path;
  }

  function storyUrl(impl, id) {
    return impl.url + '/iframe.html?id=' + encodeURIComponent(id) + '&viewMode=story';
  }

  function storyOpenUrl(impl, id) {
    return impl.url + '/?path=/story/' + encodeURIComponent(id);
  }

  function selectStory(frame, impl, id) {
    if (!frame || !frame.contentWindow) return false;
    var origin;
    try { origin = new URL(impl.url).origin; }
    catch (error) { return false; }
    frame.contentWindow.postMessage(JSON.stringify({
      key: 'storybook-channel',
      event: {
        type: 'setCurrentStory',
        args: [{ storyId: id, viewMode: 'story' }],
        from: 'canonic-workbench',
      },
    }), origin);
    return true;
  }

  function storybookEvent(message, frame, impl) {
    if (!frame || !frame.contentWindow || !impl) return null;
    try {
      if (message.source !== frame.contentWindow || message.origin !== new URL(impl.url).origin) return null;
    } catch (error) { return null; }
    var data = message.data;
    if (typeof data === 'string') {
      try { data = JSON.parse(data); } catch (error) { return null; }
    }
    if (!data || data.key !== 'storybook-channel' || !data.event) return null;
    var event = data.event;
    var type = event.type;
    if (type !== 'currentStoryWasSet' && type !== 'storyRendered') return null;
    var arg = event.args && event.args[0];
    var id = type === 'currentStoryWasSet' ? arg && arg.storyId : arg;
    return typeof id === 'string' ? { type: type, id: id } : null;
  }

  /* The frame loads an implementation through the workbench's proxy. A
     handoff or reference names the implementation's own address instead,
     which outlives this session's proxy port. */
  function upstream(impl, address) {
    if (!impl || !impl.upstream || typeof address !== 'string') return address;
    var proxied = impl.base || impl.url || '';
    try {
      var from = new URL(proxied).origin;
      if (address.indexOf(from) !== 0) return address;
      return new URL(impl.upstream).origin + address.slice(from.length);
    } catch (error) { return address; }
  }

  function pick(stories, state) {
    for (var i = 0; i < stories.length; i++) {
      if (stories[i].state === state) return stories[i];
    }
    return stories[0] || null;
  }

  window.wbLenses = {
    url: url, storyUrl: storyUrl, storyOpenUrl: storyOpenUrl,
    selectStory: selectStory, storybookEvent: storybookEvent, pick: pick, upstream: upstream,
  };
})();
