const { test } = require('node:test');
const assert = require('node:assert/strict');
const vm = require('node:vm');
const { measureRegions } = require('./capture.cjs');

function measure(pageBottom) {
  const element = (x, y, width, height) => ({ getBoundingClientRect: () => ({ x, y, width, height }) });
  const elements = {
    '.wb-topbar': element(0, 0, 1440, 44),
    '.wb-topbar-center': element(600, 0, 300, 44),
    '.wb-sidebar': element(0, 44, 250, 816),
    '.wb-sidebar .wb-search': element(8, 820, 234, 32),
    '.wb-canvas': element(250, 44, 1190, 816),
    crumb: element(0, 0, 200, 44),
    toolbar: element(600, 800, 300, 40),
    pageList: { lastElementChild: element(8, 150, 234, pageBottom - 150) },
  };
  return vm.runInNewContext('(' + measureRegions.toString() + ')()', {
    innerWidth: 1440, innerHeight: 860,
    document: {
      querySelector: selector => elements[selector],
      getElementById: id => elements[id],
    },
  });
}

test('the page-list badge follows a short list without covering the search field', () => {
  assert.equal(measure(400).regions[0].at[1], 440);
});

test('a long page list keeps its badge inside the screenshot and above search', () => {
  const y = measure(1000).regions[0].at[1];
  assert.equal(y, 796);
  assert.ok(y + 13 < 820, 'the full badge stays above the search control');
});
