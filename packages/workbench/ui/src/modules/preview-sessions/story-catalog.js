/** One canvas owns catalog lookups; isolated renderers request that catalog. */
export async function fetchStoryCatalog(implementation, title, base = location.href) {
  const url = new URL('/_workbench/stories', base);
  url.searchParams.set('implementation', implementation);
  url.searchParams.set('title', title);
  const response = await fetch(url);
  const result = await response.json();
  if (!response.ok || !result.ok || !Array.isArray(result.stories) || !result.stories.length) {
    throw new Error(result.error || 'No stories titled “' + title + '”.');
  }
  return result.stories;
}

export function createStoryCatalog(fetchStories) {
  const entries = new Map();
  return {
    stories(implementation, title) {
      const key = JSON.stringify([implementation, title]);
      let request = entries.get(key);
      if (!request) {
        request = Promise.resolve().then(() => fetchStories(implementation, title));
        entries.set(key, request);
        void request.catch(() => {
          if (entries.get(key) === request) entries.delete(key);
        });
      }
      return request;
    },
    clear() {
      entries.clear();
    },
  };
}

export function connectStoryCatalog(parent, origin, channel) {
  const pending = new Map();
  let disposed = false;
  function receive(event) {
    const data = event.data;
    if (
      event.source !== parent ||
      event.origin !== origin ||
      data?.type !== 'wb-story-catalog-response' ||
      data.channel !== channel
    )
      return;
    const job = pending.get(data.id);
    if (!job) return;
    pending.delete(data.id);
    clearTimeout(job.timer);
    if (typeof data.error === 'string') job.reject(new Error(data.error));
    else job.resolve(data.value);
  }
  window.addEventListener('message', receive);
  return {
    stories(implementation, title) {
      if (disposed) return Promise.reject(new Error('Story catalog connection closed.'));
      const id = crypto.randomUUID();
      return new Promise((resolve, reject) => {
        // An owned Storybook may take up to 120 seconds to start.
        const timer = setTimeout(() => {
          pending.delete(id);
          reject(new Error('Story catalog request timed out.'));
        }, 125000);
        pending.set(id, { resolve, reject, timer });
        parent.postMessage(
          { type: 'wb-story-catalog-request', channel, id, implementation, title },
          origin,
        );
      });
    },
    dispose() {
      disposed = true;
      window.removeEventListener('message', receive);
      for (const job of pending.values()) {
        clearTimeout(job.timer);
        job.reject(new Error('Story catalog connection closed.'));
      }
      pending.clear();
    },
  };
}
