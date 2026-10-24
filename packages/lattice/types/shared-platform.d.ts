/**
 * @file Platform APIs that pure modules may use because both minimum runtimes,
 * Node 24.12 and Chromium 133, provide them. This file is used only by the pure
 * type-check configuration and is not shipped; public types must not depend on
 * it. Anything not declared here is a type error in pure modules.
 */

interface Event {
  readonly type: string;
}

type EventListenerOrEventListenerObject =
  | ((event: Event) => void)
  | { handleEvent(event: Event): void };

interface AddEventListenerOptions {
  capture?: boolean;
  once?: boolean;
  passive?: boolean;
  signal?: AbortSignal;
}

interface EventTarget {
  addEventListener(
    type: string,
    listener: EventListenerOrEventListenerObject | null,
    options?: AddEventListenerOptions | boolean,
  ): void;
  removeEventListener(
    type: string,
    listener: EventListenerOrEventListenerObject | null,
    options?: { capture?: boolean } | boolean,
  ): void;
  dispatchEvent(event: Event): boolean;
}

interface AbortSignal extends EventTarget {
  readonly aborted: boolean;
  readonly reason: unknown;
  throwIfAborted(): void;
}

declare const AbortSignal: {
  prototype: AbortSignal;
  abort(reason?: unknown): AbortSignal;
  any(signals: AbortSignal[]): AbortSignal;
  timeout(milliseconds: number): AbortSignal;
};

interface AbortController {
  readonly signal: AbortSignal;
  abort(reason?: unknown): void;
}

declare const AbortController: {
  prototype: AbortController;
  new (): AbortController;
};

declare function queueMicrotask(callback: () => void): void;

declare function structuredClone<T>(value: T, options?: { transfer?: unknown[] }): T;

/** Present in browsers; absent in Node, so callers check it with `typeof` first. */
declare const reportError: ((error: unknown) => void) | undefined;
