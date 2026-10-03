export type Viewport = 'fit' | 'desktop' | 'mobile' | 'responsive';
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
}
export type Cleanup = void | (() => void | Promise<void>);
export interface PreviewState<T extends Record<string, unknown>> {
  label?: string;
  inputs?: Partial<T>;
  fixtures?: Record<string, unknown>;
  globals?: Record<string, unknown>;
  source?: { entry: string; export?: string };
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
  adapter: string;
  source: { entry: string; export?: string };
  styles?: string[];
  assets?: string[];
  environment?: string;
  viewports?: Viewport[];
  controls?: Record<string, Control>;
  states?: Record<string, PreviewState<NoInfer<T>>>;
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
  /** esbuild plugins run before the built-in loaders. */
  plugins?: unknown[];
  aliases?: Record<string, string>;
  resolveExtensions?: string[];
  /** Packages resolved from one project installation (React in monorepos). */
  dedupe?: string[];
  define?: Record<string, string>;
}
export function definePreview<T extends Record<string, unknown>>(preview: Preview<T>): Preview<T>;
export function defineConfig(config: WorkbenchConfig): WorkbenchConfig;
export function defineAdapter(adapter: Adapter): Adapter;
