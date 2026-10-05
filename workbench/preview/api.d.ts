/** A key of the space's sizes: one of the defaults, or one the space declares. */
export type SizeKey = 'fit' | 'laptop' | 'mobile' | 'resizable' | (string & {});
export interface PreviewContext<T extends Record<string, unknown> = Record<string, unknown>> {
  id: string;
  state: string;
  inputs: T;
  fixtures: Record<string, unknown>;
  globals: Record<string, unknown>;
  signal: AbortSignal;
  /** Report asynchronous rendering failures to the host and capture service. */
  error(error: unknown): void;
  action(name: string, ...values: unknown[]): void;
  /** Open another preview, or another state of this one, while actions are on. */
  navigate(to: PreviewLink): void;
}
/** A preview ID, or a preview and state; a state alone stays in this preview. */
export type PreviewLink = string | { preview: string; state?: string } | { preview?: string; state: string };
export type Cleanup = void | (() => void | Promise<void>);
/** A mocked response. `body` objects are sent as JSON. */
export interface MockResponse {
  status?: number;
  statusText?: string;
  headers?: Record<string, string>;
  body?: unknown;
  /** Milliseconds to wait before answering. */
  delay?: number;
  /** Never answer, to show a loading state. */
  pending?: boolean;
  /** Fail as a network error, as when offline. */
  failed?: boolean;
  /** Send this request to the network after all. */
  passthrough?: boolean;
}
/** A request as a mock handler receives it, with its body parsed. */
export interface MockRequest {
  method: string;
  url: string;
  path: string;
  query: Record<string, string>;
  headers: Record<string, string>;
  body: unknown;
  operationName: string | null;
  variables: Record<string, unknown> | null;
}
export type MockHandler<T extends Record<string, unknown> = Record<string, unknown>> =
  MockResponse | ((request: MockRequest, context: PreviewContext<T>) => MockResponse | Response | Promise<MockResponse | Response>);
/** Keys: `'GET /api/items'`, `'/api/items/*'`, `'POST /graphql Operation'`, or a full URL. */
export type RequestMocks<T extends Record<string, unknown> = Record<string, unknown>> = Record<string, MockHandler<T>>;
export interface PreviewState<T extends Record<string, unknown>> {
  label?: string;
  inputs?: Partial<T>;
  fixtures?: Record<string, unknown>;
  globals?: Record<string, unknown>;
  source?: { entry: string; export?: string };
  /** Answers for the page's fetch and XMLHttpRequest calls. */
  requests?: RequestMocks<T>;
  setup?: (context: PreviewContext<T>) => Cleanup | Promise<Cleanup>;
  play?: (context: PreviewContext<T> & { canvas: HTMLElement }) => void | Promise<void>;
  ready?: (context: PreviewContext<T>) => void | Promise<void>;
}
export interface Control {
  type: 'text' | 'number' | 'boolean' | 'select' | 'json';
  label?: string;
  options?: (string | number)[];
  min?: number;
  max?: number;
  step?: number;
}
export interface Preview<T extends Record<string, unknown> = Record<string, unknown>> extends PreviewState<T> {
  id: string;
  title?: string;
  icon?: string;
  adapter: string;
  source: { entry: string; export?: string };
  styles?: string[];
  assets?: string[];
  environment?: string;
  sizes?: SizeKey[];
  controls?: Record<string, Control>;
  states?: Record<string, PreviewState<NoInfer<T>>>;
  /** Addresses the source links or submits to, mapped to the preview each opens. */
  links?: Record<string, PreviewLink>;
  docs?: string;
}
export interface BrowserAdapter {
  mount(canvas: HTMLElement, source: unknown, context: PreviewContext, environment?: unknown): Cleanup | Promise<Cleanup>;
}
export interface Adapter {
  /** Browser module exporting mount(canvas, source, context, environment). */
  runtime: string;
  /** esbuild-compatible compiler plugins, executed only in trusted projects. */
  plugins?: unknown[];
}
export interface WorkbenchConfig {
  adapters?: Record<string, Adapter>;
  /** An environment module around every preview, or one per adapter name, relative to the project root. */
  environment?: string | Record<string, string>;
  /** esbuild plugins run before the built-in loaders. */
  plugins?: unknown[];
  aliases?: Record<string, string>;
  resolveExtensions?: string[];
  /** Packages resolved from one project installation (React in monorepos). */
  dedupe?: string[];
  define?: Record<string, string>;
}
/** One lens of a docs page: what renders its examples. Paths are relative to the definition file. */
export interface DocsLens {
  label?: string;
  adapter: string;
  /** A folder with one example per file (each file's default export), or a file with one example per named export. */
  examples: string;
  styles?: string[];
  environment?: string;
}
/** A docs page: Markdown that places examples with ```example <id>``` blocks. */
export interface Docs {
  id: string;
  title?: string;
  /** The page's icon in the page list, a kebab-case Lucide name; book-open otherwise. */
  icon?: string;
  /** The Markdown file, relative to the definition file. */
  docs: string;
  lenses?: Record<string, DocsLens>;
  /** The lens the page opens with; the first in `lenses` otherwise. */
  lens?: string;
  /** States every example receives as `context.state`. Prefer showing variations side by side. */
  states?: Record<string, { label?: string }>;
}
export function definePreview<T extends Record<string, unknown>>(preview: Preview<T>): Preview<T>;
export function defineDocs(docs: Docs): Docs & { kind: 'docs' };
export function defineConfig(config: WorkbenchConfig): WorkbenchConfig;
export function defineAdapter(adapter: Adapter): Adapter;
