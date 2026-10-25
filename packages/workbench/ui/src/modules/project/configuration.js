/* Existing handwritten configuration rules and HTTP operations. No presentation state. */
(function () {
  /* A page's `sizes` as written: a list, or null when it has none and so
     supports every size of the space. */
  function sizeList(page) {
    var raw = page.sizes;
    if (raw === undefined || raw === null || raw === '') return null;
    return Array.isArray(raw) ? raw : [raw];
  }

  /* The keys a page supports, out of `spaceKeys`. The server resolves and
     checks sizes; this only reads which listed keys are on. */
  function pageSizes(page, spaceKeys) {
    var listed = sizeList(page);
    if (!listed) return spaceKeys.slice();
    return listed.filter(function (entry) {
      return typeof entry === 'string';
    });
  }

  /* Turns one of the space's sizes on or off for a page, keeping its other
     entries — sizes of its own included — as they are written. A page left
     with exactly the space's sizes loses its list. */
  function toggleSize(page, key, spaceKeys) {
    var listed = sizeList(page) || spaceKeys.slice();
    var at = listed.indexOf(key);
    var next =
      at === -1
        ? listed.concat([key])
        : listed.filter(function (entry) {
            return entry !== key;
          });
    if (!next.length) return;
    var keys = next.filter(function (entry) {
      return typeof entry === 'string';
    });
    var every =
      keys.length === next.length &&
      keys.length === spaceKeys.length &&
      spaceKeys.every(function (candidate) {
        return keys.indexOf(candidate) !== -1;
      });
    if (every) delete page.sizes;
    else page.sizes = next;
  }

  /* A group stays a group while its name is being retyped. */
  function isGroup(entry) {
    return entry.group !== undefined && entry.group !== null;
  }

  function newPage() {
    return { label: 'Untitled page', src: 'pages/untitled.html' };
  }

  /* A group as the YAML needs it: a name and at least one page. Groups hold
     pages only; they don't nest. */
  function newGroup() {
    return { group: 'New group', items: [newPage()] };
  }

  /* The first reason the draft can't be saved, or null. */
  function validate(collections) {
    var problem = null;
    (collections || []).some(function (collection, index) {
      if (!String(collection.name || '').trim()) {
        problem = 'Collection ' + (index + 1) + ' needs a name.';
        return true;
      }
      function visit(entries) {
        return (entries || []).some(function (entry) {
          if (isGroup(entry)) {
            if (!String(entry.group || '').trim() || !(entry.items || []).length) {
              problem = 'Every group needs a name and at least one page.';
              return true;
            }
            return visit(entry.items);
          }
          if (!String(entry.label || '').trim() || !String(entry.src || '').trim()) {
            problem = 'Every page needs a label and source path.';
            return true;
          }
          return false;
        });
      }
      return visit(collection.items);
    });
    return problem;
  }

  function read() {
    return fetch('/_workbench/config-file').then(function (response) {
      return response.json().then(function (result) {
        if (!response.ok || !result.ok)
          throw new Error(result.error || 'Could not read workbench.yaml.');
        return result;
      });
    });
  }

  function write(collections) {
    return fetch('/_workbench/config-file', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify({ collections: collections }),
    }).then(function (response) {
      return response.json().then(function (result) {
        if (!response.ok || !result.ok)
          throw new Error(result.error || 'Could not save workbench.yaml.');
        return result;
      });
    });
  }

  function postSizes(body) {
    return fetch('/_workbench/sizes', {
      method: 'POST',
      headers: { 'Content-Type': 'application/json' },
      body: JSON.stringify(body),
    }).then(function (response) {
      return response
        .json()
        .catch(function () {
          return {};
        })
        .then(function (result) {
          if (!response.ok || !result.ok)
            throw new Error(result.error || 'Could not save workbench.yaml.');
          return result;
        });
    });
  }

  window.wbProjectConfiguration = {
    postSizes,
    pageSizes,
    toggleSize,
    isGroup,
    newPage,
    newGroup,
    validate,
    read,
    write,
  };
})();
