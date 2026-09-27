/* Handoff prompt
   --------------
   Turns what the canvas knows into the paragraph an agent is handed.

   The screenshot alone is a poor brief. An agent reading it has to work out
   which pixels are the design and which are the notes on it, guess what a red
   box is drawn around, and read handwriting-sized text off a PNG. Every one of
   those is something the workbench already knows exactly, so it says it in
   words and lets the picture be corroboration rather than evidence.

   Three things this has to establish, in order:
   - the marks are not part of the design. Red belongs to the workbench, not
     the reviewed product, and a red rectangle over a form is an instruction,
     not a border to implement.
   - where the screen came from and at what width, so the agent edits the file
     that was actually being looked at, at the breakpoint it was looked at.
   - what each mark is attached to, by element, so "make this full width" has a
     referent that isn't a coordinate.

   No vscode import and no node API: this is string work, and keeping it that
   way means the wording can be checked with `node handoff.js` rather than by
   reloading an editor.
*/

var TYPE_NAMES = {
  arrow: 'Arrow',
  line: 'Line',
  rect: 'Rectangle',
  ellipse: 'Circle',
  draw: 'Scribble',
  text: 'Note',
  comment: 'Comment',
};

function round(n) {
  return Math.round(n);
}

function at(p) {
  return '(' + round(p.x) + ', ' + round(p.y) + ')';
}

function size(box) {
  return round(box.w) + ' × ' + round(box.h);
}

/* One line per mark, numbered the way they were drawn. The element under a
   mark is the whole point of the list, so it goes at the end of the line where
   it reads as the subject rather than as a coordinate footnote. */
function markLine(mark, i) {
  var name = TYPE_NAMES[mark.type] || mark.type;
  var line = i + 1 + '. ' + name;

  if (mark.type === 'text') {
    line += ' at ' + at(mark.box) + ': “' + mark.text + '”';
    return line;
  }

  if (mark.type === 'comment') {
    line += ' at ' + at(mark.box) + ', ' + size(mark.box) + ': “' + mark.text + '”';
    if (mark.target) line += ' — on ' + mark.target;
    return line;
  }

  if (mark.type === 'arrow') line += ' pointing at ' + at(mark.to);
  else if (mark.type === 'line') line += ' from ' + at(mark.from) + ' to ' + at(mark.to);
  else line += ' at ' + at(mark.box) + ', ' + size(mark.box);

  if (mark.target) line += ' — on ' + mark.target;
  return line;
}

/* Notes and comments carry words, and those words are the request.
   Pulled out so the agent doesn't have to reconstruct the ask from a list of
   shapes. */
function notes(marks) {
  return marks.filter(function (m) {
    return (m.type === 'text' || m.type === 'comment') && m.text;
  });
}

function prompt(payload) {
  var marks = payload.marks || [];
  var out = [];

  out.push('Here is a screen from the Canonic workbench.');
  out.push('');
  out.push('- Screen: ' + payload.label + ' — `' + payload.src + '`');
  /* Only when it isn't the default one — saying "State: Default" on every
     screen without states would be noise. */
  if (payload.state) out.push('- State: ' + payload.state);
  /* Seen through a lens: which story, and which implementation at which
     address. Then where the code is, whichever lens was up — a design with
     a known implementation is the point of the bridge. */
  if (payload.story) out.push('- Story: ' + payload.story.name + ' — `' + payload.story.id + '`');
  if (payload.lens) out.push('- Implementation: ' + payload.lens.label + ' — `' + payload.lens.url + '`');
  if (payload.code && payload.code.length) {
    out.push('- Source: ' + payload.code.map(function (file) {
      return '`' + file + '`';
    }).join(', '));
  }
  out.push('- Width: ' + payload.width + ' (frame is ' + payload.frame.w + ' × ' + payload.frame.h + ' CSS px)');
  out.push('- Screenshot: `' + payload.file + '`');
  out.push('');

  if (payload.lens) {
    out.push(
      'The screenshot is the implementation at that address, seen through the workbench — ' +
        'the design it should match is `' + payload.src + '`.'
    );
    out.push('');
  }

  if (!marks.length) {
    out.push(
      payload.lens
        ? 'Nothing is marked up — the screenshot is the implementation as it stands.'
        : 'Nothing is marked up — the screenshot is the design as it stands.'
    );
    return out.join('\n');
  }

  out.push(
    'The screenshot has ' + marks.length + ' ' + (marks.length === 1 ? 'mark' : 'marks') +
      ' drawn over it. **Everything red in the image is annotation, not design** — ' +
      'arrows, boxes, circles, scribbles and the red text are notes about the screen, ' +
      'drawn on top of it. Nothing red is something to build.'
  );
  out.push('');
  out.push(
    'Rather than reading them off the picture, here they are. Coordinates are CSS ' +
      'pixels from the top-left of the frame at the width above, and the element ' +
      'named on each line is the one under that mark in the live DOM:'
  );
  out.push('');
  marks.forEach(function (mark, i) {
    out.push(markLine(mark, i));
  });

  /* Marks that landed on nothing are worth admitting to: an arrow into empty
     space is usually pointing at a gap, and an agent told the list is exact
     shouldn't have to wonder why one line is short. */
  var unplaced = marks.filter(function (m) {
    return m.type !== 'text' && !m.target;
  }).length;
  if (unplaced && payload.inspected === false) {
    /* Not empty space — a page served from elsewhere, whose DOM the
       workbench can't read into. Say that rather than mislead. */
    out.push('');
    out.push(
      '(The elements under the marks couldn’t be read — the page is served from elsewhere — ' +
        'so go by the coordinates and the screenshot.)'
    );
  } else if (unplaced) {
    out.push('');
    out.push(
      '(' + unplaced + ' of them ' + (unplaced === 1 ? 'is' : 'are') +
        ' over empty space rather than an element — check the screenshot for what ' +
        (unplaced === 1 ? 'it means' : 'they mean') + '.)'
    );
  }

  var asks = notes(marks);
  if (asks.length) {
    out.push('');
    out.push('In words, the notes say:');
    asks.forEach(function (m) {
      out.push('- “' + m.text + '”');
    });
  }

  return out.join('\n');
}

module.exports = { prompt: prompt, markLine: markLine };

/* `node handoff.js` prints the prompt for a sample canvas, which is the
   fastest way to see whether a wording change reads well. */
if (require.main === module) {
  console.log(
    prompt({
      label: 'Sign in',
      src: 'pages/sign-in.html',
      state: 'Wrong password',
      width: 'Mobile · iPhone 15 Pro, 393 × 852',
      frame: { w: 393, h: 852 },
      file: '.canonic/.handoffs/sign-in.png',
      marks: [
        { type: 'arrow', from: { x: 120, y: 210 }, to: { x: 168, y: 254 }, target: 'fh-button.primary “Sign in”' },
        { type: 'rect', box: { x: 24, y: 300, w: 342, h: 96 }, target: 'fh-field-text-input#email' },
        { type: 'ellipse', box: { x: 300, y: 96, w: 40, h: 40 }, target: null },
        { type: 'text', box: { x: 30, y: 410 }, text: 'make this full width' },
      ],
    })
  );
}
