/* Collection traversal, indexing and initial-page rules. The classic shell and
   page-list view share this existing API until the project capability port. */
(function () {
  function isGroup(entry) {
    return !!(entry && entry.group);
  }

  function pagesIn(entries) {
    var out = [];
    entries.forEach(function (entry) {
      if (isGroup(entry)) out = out.concat(pagesIn(entry.items));
      else out.push(entry);
    });
    return out;
  }

  function firstPick(pages) {
    var item = pages[0];
    return item ? { src: item.src, scenario: null } : null;
  }

  function firstPage(collections) {
    for (var i = 0; i < collections.length; i++) {
      var pick = firstPick(pagesIn(collections[i].items));
      if (pick) return pick;
    }
    return null;
  }

  function index(collections) {
    var out = {};
    (collections || []).forEach(function (g) {
      pagesIn(g.items).forEach(function (item) {
        item.collection = g.name;
        item.icon = item.icon || g.icon;
        out[item.src] = item;
      });
    });
    return out;
  }

  window.wbPageList = {
    index: index,
    firstPage: firstPage,
    firstPick: firstPick,
    pagesIn: pagesIn,
    isGroup: isGroup,
  };
})();
