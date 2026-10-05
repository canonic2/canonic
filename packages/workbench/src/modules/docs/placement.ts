/* Matches a docs page's placed examples with what one lens exports. Every
   placement keeps its panel in every lens so switching lenses never moves the
   text: an example the lens lacks becomes a placeholder, and an example the
   Markdown never places is a problem and is not shown. */

import type { ExamplePlacement } from './docs-markdown.ts';

export type PanelStatus = 'ready' | 'missing' | 'invalid';

export interface ExamplePanel {
  id: string;
  label: string;
  caption: string | null;
  /** 1-based line of the placement in the Markdown file. */
  line: number;
  /**
   * `ready`: the lens exports it. `missing`: placed, but this lens has no such
   * example. `invalid`: a duplicate or malformed placement.
   */
  status: PanelStatus;
}

export interface LensMatch {
  panels: ExamplePanel[];
  /** IDs the lens exports that no placement names, in the lens's order. */
  unplaced: string[];
}

export function matchLens(placements: readonly ExamplePlacement[], exported: readonly string[]): LensMatch {
  const available = new Set(exported);
  const placed = new Set<string>();
  const panels = placements.map((placement): ExamplePanel => {
    if (!placement.duplicate) placed.add(placement.id);
    return {
      id: placement.id,
      label: placement.label,
      caption: placement.caption,
      line: placement.line,
      status: placement.duplicate ? 'invalid' : available.has(placement.id) ? 'ready' : 'missing',
    };
  });
  return { panels, unplaced: exported.filter(id => !placed.has(id)) };
}
