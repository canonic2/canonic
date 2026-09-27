var assert = require('node:assert/strict');
var test = require('node:test');
var handoff = require('./handoff');

test('serializes every markup type with its geometry and target', function () {
  var text = handoff.prompt({
    label: 'Example',
    src: 'preview/example.html',
    width: 'Fit',
    frame: { w: 800, h: 600 },
    file: '.canonic/.handoffs/example.png',
    marks: [
      { type: 'arrow', from: { x: 1, y: 2 }, to: { x: 3, y: 4 }, target: 'button.primary' },
      { type: 'line', from: { x: 5, y: 6 }, to: { x: 7, y: 8 }, target: 'div.rule' },
      { type: 'rect', box: { x: 9, y: 10, w: 11, h: 12 }, target: 'section.card' },
      { type: 'ellipse', box: { x: 13, y: 14, w: 15, h: 16 }, target: 'span.avatar' },
      { type: 'draw', box: { x: 17, y: 18, w: 19, h: 20 }, target: 'nav.sidebar' },
      { type: 'text', box: { x: 21, y: 22, w: 23, h: 24 }, text: 'Freeform note' },
      { type: 'comment', box: { x: 25, y: 26, w: 27, h: 28 }, text: 'Written comment', target: 'button.company' },
    ],
  });

  assert.match(text, /1\. Arrow pointing at \(3, 4\) — on button\.primary/);
  assert.match(text, /2\. Line from \(5, 6\) to \(7, 8\) — on div\.rule/);
  assert.match(text, /3\. Rectangle at \(9, 10\), 11 × 12 — on section\.card/);
  assert.match(text, /4\. Circle at \(13, 14\), 15 × 16 — on span\.avatar/);
  assert.match(text, /5\. Scribble at \(17, 18\), 19 × 20 — on nav\.sidebar/);
  assert.match(text, /6\. Note at \(21, 22\): “Freeform note”/);
  assert.match(text, /7\. Comment at \(25, 26\), 27 × 28: “Written comment” — on button\.company/);
});

test('includes both freeform notes and comments in the written request summary', function () {
  var text = handoff.prompt({
    label: 'Example',
    src: 'preview/example.html',
    width: 'Fit',
    frame: { w: 800, h: 600 },
    file: '.canonic/.handoffs/example.png',
    marks: [
      { type: 'text', box: { x: 10, y: 20, w: 100, h: 30 }, text: 'Freeform note' },
      { type: 'comment', box: { x: 30, y: 40, w: 200, h: 80 }, text: 'Written comment', target: null },
    ],
  });

  assert.match(text, /In words, the notes say:/);
  assert.match(text, /- “Freeform note”/);
  assert.match(text, /- “Written comment”/);
  assert.match(text, /\(1 of them is over empty space/);
});

/* The prompt for a design, word for word: lenses added lines for their own
   case and none for this one. */
test('reads exactly as it did for a design without code pointers', function () {
  var text = handoff.prompt({
    label: 'Sign in',
    src: 'pages/sign-in.html',
    state: 'Wrong password',
    width: 'Mobile · iPhone 15 Pro, 393 × 852',
    frame: { w: 393, h: 852 },
    file: '.canonic/.handoffs/sign-in-error.png',
    marks: [
      { type: 'rect', box: { x: 9, y: 10, w: 11, h: 12 }, target: 'section.card' },
      { type: 'comment', box: { x: 25, y: 26, w: 27, h: 28 }, text: 'Tighter', target: null },
    ],
  });

  assert.equal(text, [
    'Here is a screen from the Canonic workbench.',
    '',
    '- Screen: Sign in — `pages/sign-in.html`',
    '- State: Wrong password',
    '- Width: Mobile · iPhone 15 Pro, 393 × 852 (frame is 393 × 852 CSS px)',
    '- Screenshot: `.canonic/.handoffs/sign-in-error.png`',
    '',
    'The screenshot has 2 marks drawn over it. **Everything red in the image is annotation, not design** — arrows, boxes, circles, scribbles and the red text are notes about the screen, drawn on top of it. Nothing red is something to build.',
    '',
    'Rather than reading them off the picture, here they are. Coordinates are CSS pixels from the top-left of the frame at the width above, and the element named on each line is the one under that mark in the live DOM:',
    '',
    '1. Rectangle at (9, 10), 11 × 12 — on section.card',
    '2. Comment at (25, 26), 27 × 28: “Tighter”',
    '',
    '(1 of them is over empty space rather than an element — check the screenshot for what it means.)',
    '',
    'In words, the notes say:',
    '- “Tighter”',
  ].join('\n'));
});

test('says which implementation, story and source a lens shot is of', function () {
  var text = handoff.prompt({
    label: 'Button',
    src: 'preview/components-button.html',
    state: 'Icon Only',
    story: { id: 'components-button--icon-only', name: 'Icon Only' },
    lens: { key: 'storybook', label: 'Storybook', kind: 'storybook', url: 'http://localhost:6006/iframe.html?id=components-button--icon-only&viewMode=story' },
    code: ['/repo/packages/ui/src/button.tsx', '/repo/packages/ui/src/button.stories.tsx'],
    inspected: false,
    width: 'Fit',
    frame: { w: 800, h: 600 },
    file: '.canonic/.handoffs/components-button-icon-only-storybook.png',
    marks: [{ type: 'rect', box: { x: 9, y: 10, w: 11, h: 12 }, target: null }],
  });

  assert.match(text, /^- State: Icon Only$/m);
  assert.match(text, /^- Story: Icon Only — `components-button--icon-only`$/m);
  assert.match(text, /^- Implementation: Storybook — `http:\/\/localhost:6006\/iframe\.html\?id=components-button--icon-only&viewMode=story`$/m);
  assert.match(text, /^- Source: `\/repo\/packages\/ui\/src\/button\.tsx`, `\/repo\/packages\/ui\/src\/button\.stories\.tsx`$/m);
  assert.match(text, /The screenshot is the implementation at that address, seen through the workbench — the design it should match is `preview\/components-button\.html`\./);
  assert.match(text, /\(The elements under the marks couldn’t be read — the page is served from elsewhere — so go by the coordinates and the screenshot\.\)/);
  assert.doesNotMatch(text, /over empty space/);
});

test('names the source on a design that has one, and nothing else new', function () {
  var text = handoff.prompt({
    label: 'Sign in',
    src: 'pages/sign-in.html',
    code: ['/repo/src/pages/login'],
    width: 'Fit',
    frame: { w: 800, h: 600 },
    file: '.canonic/.handoffs/sign-in.png',
    marks: [],
  });

  assert.match(text, /^- Source: `\/repo\/src\/pages\/login`$/m);
  assert.doesNotMatch(text, /Implementation:/);
  assert.match(text, /Nothing is marked up — the screenshot is the design as it stands\./);
});

test('an unmarked lens shot is the implementation as it stands', function () {
  var text = handoff.prompt({
    label: 'Sign in',
    src: 'pages/sign-in.html',
    lens: { key: 'dev', label: 'Dev', kind: 'url', url: 'http://localhost:3710/' },
    width: 'Fit',
    frame: { w: 800, h: 600 },
    file: '.canonic/.handoffs/sign-in-dev.png',
    marks: [],
  });
  assert.match(text, /^- Implementation: Dev — `http:\/\/localhost:3710\/`$/m);
  assert.match(text, /Nothing is marked up — the screenshot is the implementation as it stands\./);
});
