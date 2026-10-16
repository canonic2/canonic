// Data exchanged between the docs server and its browser page.
/** What a page's script mounts, answered after the page itself: see `bundleInfo`. */
export interface BundleInfo {
  module: string | null;
  stylesheet: string | null;
  /** The IDs the lens exports; a placed example that isn't one says it's missing. */
  examples: string[];
  /** Why nothing mounts: the build failed, or examples can't run here. */
  error: string | null;
  /** The page's revision, which the page then polls against. */
  revision: string;
}

/** What the docs page script reads from window.__workbenchDocs. */
export interface DocsPageOptions {
  /** The docs page's src, relative to the project root. */
  page: string;
  /** The lens rendering the examples, and its label, or null when the page has none. */
  lens: string | null;
  lensLabel: string | null;
  state: string | null;
  /** The lens's examples bundle, where to ask for it (a deferred page), or null when nothing is mounted. */
  bundle: { module: string } | { info: string } | null;
  /** Where Show code asks for an example's source; the example ID is appended. */
  sourceUrl: string;
  /** Answers the page's current revision, so an edit reloads it; empty where nothing changes. */
  revisionUrl: string;
  /** Empty on a deferred page until its bundle answers. */
  revision: string;
}
