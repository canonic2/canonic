import type { ExportRequest } from './request.ts';
interface Reference { page: string; state: string; size: string; width: number; height: number; variant: string; docsPage?: boolean; selector?: string; url: string; [key: string]: unknown }
export function selectCaptures<T extends Reference>(captures: T[], request: ExportRequest): T[] {
  const current = request.variants === 'current' ? request.current : undefined;
  const seen = new Set<string>();
  const firstState = captures.find(c => !c.docsPage)?.state;
  const firstDocsLens = captures.find(c => c.docsPage) && new URL(captures.find(c => c.docsPage)!.url).searchParams.get('lens');
  return captures.flatMap(reference => {
    if ((request.format === 'pdf' || current) && reference.selector) return [];
    if (reference.docsPage) {
      const lens = current ? current.lens || firstDocsLens : request.lens;
      if (lens && new URL(reference.url).searchParams.get('lens') !== lens) return [];
    } else {
      if (current && reference.state !== (current.state || firstState)) return [];
      if (!current && request.states.length && !request.states.includes(reference.state)) return [];
      if (!current && request.sizes.length && !request.sizes.includes(reference.size)) return [];
    }
    let selected = reference;
    if (current && !reference.docsPage) selected = { ...reference, width: current.width, height: current.height,
      size: current.size || 'current', variant: reference.state + '-current' };
    const key = selected.page + ':' + selected.variant;
    if (seen.has(key)) return [];
    seen.add(key);
    return [selected];
  });
}
