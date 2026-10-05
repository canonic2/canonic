var assert = require('node:assert/strict');
var test = require('node:test');
var handoff = require('./handoff');

test('an authored lens label appears in handoffs without treating it as an implementation', function () {
  var text = handoff.prompt({ label: 'Button', src: 'button.workbench.ts', lensLabel: 'Reference',
    size: 'Fit', frame: { w: 800, h: 600 }, file: 'button.jpg', annotations: [] });
  assert.match(text, /- Lens: Reference/);
  assert.doesNotMatch(text, /- Implementation:/);
  assert.doesNotMatch(text, /the design it should match/);
});

test('serializes every annotation type with its geometry and target', function () {
  var text = handoff.prompt({
    label: 'Example',
    src: 'preview/example.html',
    size: 'Fit',
    frame: { w: 800, h: 600 },
    file: '.canonic/.handoffs/example.png',
    annotations: [
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
    size: 'Fit',
    frame: { w: 800, h: 600 },
    file: '.canonic/.handoffs/example.png',
    annotations: [
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
    size: 'Mobile, 393 × 852',
    frame: { w: 393, h: 852 },
    file: '.canonic/.handoffs/sign-in-error.png',
    annotations: [
      { type: 'rect', box: { x: 9, y: 10, w: 11, h: 12 }, target: 'section.card' },
      { type: 'comment', box: { x: 25, y: 26, w: 27, h: 28 }, text: 'Tighter', target: null },
    ],
  });

  assert.equal(text, [
    'Here is a page from Workbench.',
    '',
    '- Page: Sign in — `pages/sign-in.html`',
    '- State: Wrong password',
    '- Size: Mobile, 393 × 852 (artboard is 393 × 852 CSS px)',
    '- Screenshot: `.canonic/.handoffs/sign-in-error.png`',
    '',
    'The screenshot has 2 annotations drawn over it. **Everything red in the image is an annotation, not the design** — arrows, boxes, circles, scribbles and the red text are notes about the page, drawn on top of it. Nothing red is something to build.',
    '',
    'Rather than reading them off the picture, here they are. Coordinates are CSS pixels from the top-left of the artboard at the size above, and the element named on each line is the one under that annotation in the live DOM:',
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
    size: 'Fit',
    frame: { w: 800, h: 600 },
    file: '.canonic/.handoffs/components-button-icon-only-storybook.png',
    annotations: [{ type: 'rect', box: { x: 9, y: 10, w: 11, h: 12 }, target: null }],
  });

  assert.match(text, /^- State: Icon Only$/m);
  assert.match(text, /^- Story: Icon Only — `components-button--icon-only`$/m);
  assert.match(text, /^- Implementation: Storybook — `http:\/\/localhost:6006\/iframe\.html\?id=components-button--icon-only&viewMode=story`$/m);
  assert.match(text, /^- Source: `\/repo\/packages\/ui\/src\/button\.tsx`, `\/repo\/packages\/ui\/src\/button\.stories\.tsx`$/m);
  assert.match(text, /The screenshot is the implementation at that address, seen through the workbench — the design it should match is `preview\/components-button\.html`\./);
  assert.match(text, /\(The elements under the annotations couldn’t be read — the page is served from elsewhere — so go by the coordinates and the screenshot\.\)/);
  assert.doesNotMatch(text, /over empty space/);
});

test('names the source on a design that has one, and nothing else new', function () {
  var text = handoff.prompt({
    label: 'Sign in',
    src: 'pages/sign-in.html',
    code: ['/repo/src/pages/login'],
    size: 'Fit',
    frame: { w: 800, h: 600 },
    file: '.canonic/.handoffs/sign-in.png',
    annotations: [],
  });

  assert.match(text, /^- Source: `\/repo\/src\/pages\/login`$/m);
  assert.doesNotMatch(text, /Implementation:/);
  assert.match(text, /Nothing is annotated — the screenshot is the design as it stands\./);
});

test('an unannotated lens shot is the implementation as it stands', function () {
  var text = handoff.prompt({
    label: 'Sign in',
    src: 'pages/sign-in.html',
    lens: { key: 'dev', label: 'Dev', kind: 'url', url: 'http://localhost:3710/' },
    size: 'Fit',
    frame: { w: 800, h: 600 },
    file: '.canonic/.handoffs/sign-in-dev.png',
    annotations: [],
  });
  assert.match(text, /^- Implementation: Dev — `http:\/\/localhost:3710\/`$/m);
  assert.match(text, /Nothing is annotated — the screenshot is the implementation as it stands\./);
});

test('a handoff through a docs lens names the Markdown, lens, and the examples in view with their code', function () {
  var text = handoff.prompt({
    file: '.canonic/.handoffs/card.jpg', src: 'pages/card.html', label: 'Card', state: null, size: 'Docs, filling the canvas',
    frame: { w: 1200, h: 800 }, annotations: [],
    lens: { key: 'dark', label: 'Dark', kind: 'docs', url: 'http://127.0.0.1:3579/docs/card.md?lens=dark' },
    docs: { lens: 'dark', markdown: 'docs/card.md', lensLabel: 'Dark', examples: [
      { id: 'basic', status: 'ready', file: 'docs/card.examples.ts (basic)' },
      { id: 'nested', status: 'missing' },
    ] },
  });
  assert.match(text, /- Docs: `docs\/card.md`, examples rendered by Dark/);
  assert.match(text, /- Examples in view: basic — `docs\/card.examples.ts \(basic\)`; nested \(missing\)/);
  assert.doesNotMatch(text, /Implementation:|the design it should match/);
  assert.match(text, /- View: the docs fill the canvas; the part in view is 1200 × 800 CSS px/);
  assert.doesNotMatch(text, /artboard is/);
  assert.match(text, /the screenshot is the docs as they stand/);
});
