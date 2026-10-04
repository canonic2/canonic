/* Reads a docs page's Markdown for what Workbench needs before rendering it:
   front matter to strip, the example placements (fenced `example <id>` blocks
   and their options), and the headings that name and anchor them. Rendering
   the Markdown to HTML is separate; this module only scans it. Fences follow
   CommonMark: three or more backticks or tildes, indented at most three spaces,
   closed by a run of the same character at least as long. */

import { isExampleId } from './example-ids.ts';

export interface DocsHeading {
  level: number;
  text: string;
  /** GitHub-style anchor, unique within the page. */
  anchor: string;
  /** 1-based line in the original file. */
  line: number;
}

export interface ExamplePlacement {
  id: string;
  /** 1-based line of the opening fence in the original file. */
  line: number;
  caption: string | null;
  /** The nearest heading above the placement, if any. */
  heading: DocsHeading | null;
  /** The sidebar label: the heading's text, or the ID when there is no heading or another example shares it. */
  label: string;
  /** A second placement of an ID already placed; it renders as an error panel. */
  duplicate: boolean;
}

export interface DocsProblem {
  line: number;
  message: string;
}

export interface DocsMarkdown {
  /** The Markdown without front matter. */
  body: string;
  /** How many lines of front matter were removed, so body line + offset = file line. */
  bodyLineOffset: number;
  headings: DocsHeading[];
  placements: ExamplePlacement[];
  problems: DocsProblem[];
}

/** Keys an example block may set. */
const EXAMPLE_KEYS = new Set(['caption']);

const FENCE_OPEN = /^( {0,3})(`{3,}|~{3,})(.*)$/;
const ATX = /^ {0,3}(#{1,6})(?:[ \t]+(.*?))?(?:[ \t]+#+)?[ \t]*$/;
const SETEXT = /^ {0,3}(=+|-+)[ \t]*$/;

export function readDocsMarkdown(source: string): DocsMarkdown {
  const text = source.replace(/^﻿/, '').replace(/\r\n?/g, '\n');
  const { body, offset } = stripFrontMatter(text);
  const lines = body.split('\n');
  const headings: DocsHeading[] = [];
  const placements: ExamplePlacement[] = [];
  const problems: DocsProblem[] = [];
  const anchors = new Map<string, number>();
  const placed = new Set<string>();

  let fence: { char: string; length: number; start: number; example: PendingExample | null } | null = null;
  let previousParagraph: { text: string; line: number } | null = null;

  for (let index = 0; index < lines.length; index++) {
    const line = lines[index] ?? '';
    const fileLine = index + 1 + offset;

    if (fence) {
      if (isClosingFence(line, fence.char, fence.length)) {
        if (fence.example) placements.push(finishExample(fence.example, headings, placed, problems));
        fence = null;
      } else if (fence.example) {
        readOption(fence.example, line, fileLine, problems);
      }
      continue;
    }

    const open = FENCE_OPEN.exec(line);
    if (open && !(open[2]!.startsWith('`') && open[3]!.includes('`'))) {
      const info = open[3]!.trim();
      fence = { char: open[2]!.charAt(0), length: open[2]!.length, start: fileLine, example: startExample(info, fileLine, problems) };
      previousParagraph = null;
      continue;
    }

    const atx = ATX.exec(line);
    if (atx) {
      headings.push(heading(atx[1]!.length, atx[2] ?? '', fileLine, anchors));
      previousParagraph = null;
      continue;
    }

    const setext = SETEXT.exec(line);
    if (setext && previousParagraph) {
      headings.push(heading(setext[1]!.startsWith('=') ? 1 : 2, previousParagraph.text, previousParagraph.line, anchors));
      previousParagraph = null;
      continue;
    }

    previousParagraph = line.trim() && !/^ {4}/.test(line) ? { text: line.trim(), line: fileLine } : null;
  }

  if (fence) {
    if (fence.example) {
      problems.push({ line: fence.start, message: `example “${fence.example.id}” has no closing fence` });
      placements.push(finishExample(fence.example, headings, placed, problems));
    }
  }

  labelPlacements(placements);
  return { body, bodyLineOffset: offset, headings, placements, problems };
}

/** GitHub's heading anchor: lowercase, punctuation dropped, spaces to hyphens. */
export function headingAnchor(text: string): string {
  return plainText(text)
    .toLowerCase()
    .replace(/[^\p{L}\p{N}\s_-]/gu, '')
    .trim()
    .replace(/\s/g, '-');
}

interface PendingExample {
  id: string;
  line: number;
  caption: string | null;
  invalid: boolean;
}

function stripFrontMatter(text: string): { body: string; offset: number } {
  if (!/^---[ \t]*\n/.test(text)) return { body: text, offset: 0 };
  const lines = text.split('\n');
  for (let index = 1; index < lines.length; index++) {
    if (/^(---|\.\.\.)[ \t]*$/.test(lines[index] ?? '')) {
      return { body: lines.slice(index + 1).join('\n'), offset: index + 1 };
    }
  }
  return { body: text, offset: 0 };
}

function isClosingFence(line: string, char: string, length: number): boolean {
  const match = /^ {0,3}(`+|~+)[ \t]*$/.exec(line);
  return !!match && match[1]!.charAt(0) === char && match[1]!.length >= length;
}

function startExample(info: string, line: number, problems: DocsProblem[]): PendingExample | null {
  const words = info.split(/\s+/).filter(Boolean);
  if (words[0] !== 'example') return null;
  const id = words[1] ?? '';
  if (!id) {
    problems.push({ line, message: 'example block names no example; write ```example <id>' });
    return { id: '', line, caption: null, invalid: true };
  }
  if (words.length > 2) problems.push({ line, message: `example “${id}”: unexpected “${words.slice(2).join(' ')}” after the ID` });
  if (!isExampleId(id)) {
    problems.push({ line, message: `example id “${id}” must be kebab-case` });
    return { id, line, caption: null, invalid: true };
  }
  return { id, line, caption: null, invalid: false };
}

function readOption(example: PendingExample, line: string, fileLine: number, problems: DocsProblem[]): void {
  if (!line.trim()) return;
  const match = /^\s*([A-Za-z][\w-]*)\s*:\s*(.*?)\s*$/.exec(line);
  if (!match) {
    problems.push({ line: fileLine, message: `example “${example.id}”: expected “key: value”, found “${line.trim()}”` });
    return;
  }
  const key = match[1]!;
  if (!EXAMPLE_KEYS.has(key)) {
    problems.push({ line: fileLine, message: `example “${example.id}”: unknown key “${key}”` });
    return;
  }
  if (key === 'caption') example.caption = match[2] || null;
}

function finishExample(example: PendingExample, headings: DocsHeading[], placed: Set<string>, problems: DocsProblem[]): ExamplePlacement {
  const duplicate = !example.invalid && placed.has(example.id);
  if (duplicate) problems.push({ line: example.line, message: `example “${example.id}” is placed more than once` });
  if (!example.invalid) placed.add(example.id);
  return {
    id: example.id,
    line: example.line,
    caption: example.caption,
    heading: headings.at(-1) ?? null,
    label: example.id,
    duplicate: duplicate || example.invalid,
  };
}

function labelPlacements(placements: ExamplePlacement[]): void {
  const perHeading = new Map<DocsHeading, number>();
  for (const placement of placements) {
    if (placement.heading && !placement.duplicate) perHeading.set(placement.heading, (perHeading.get(placement.heading) ?? 0) + 1);
  }
  for (const placement of placements) {
    const shared = placement.heading ? (perHeading.get(placement.heading) ?? 0) > 1 : true;
    placement.label = placement.heading && !shared ? placement.heading.text : placement.id;
  }
}

function heading(level: number, raw: string, line: number, anchors: Map<string, number>): DocsHeading {
  const text = plainText(raw);
  const base = headingAnchor(raw) || 'section';
  const count = anchors.get(base) ?? 0;
  anchors.set(base, count + 1);
  return { level, text, anchor: count ? `${base}-${count}` : base, line };
}

/** Heading text without inline Markdown: code spans, links, images, emphasis, and HTML tags. */
function plainText(raw: string): string {
  return raw
    .replace(/`([^`]*)`/g, '$1')
    .replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
    .replace(/<[^>]+>/g, '')
    .replace(/(\*\*|__|\*|_|~~)(.+?)\1/g, '$2')
    .trim();
}
