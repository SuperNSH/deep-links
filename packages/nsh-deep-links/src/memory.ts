import type { DeepLinksBackend } from "./backend";
import { MAX_PENDING } from "./core";
import { type DeepLinksError, type DeepLinksErrorCode, deepLinksError } from "./errors";
import type { LinkKind } from "./links";

/** A backend the test scripts, the mirror of the crate's `FakeBackend`. */
export interface MemoryDeepLinks extends DeepLinksBackend {
  /** Sets the URL the app was started with. */
  setInitial(url: string | null): void;
  /** Makes reading the launch URL fail with `code`. */
  failInitial(code: DeepLinksErrorCode): void;
  /**
   * Hands a URL over: to the waiting call at once, else to the queue, the oldest dropped past
   * `MAX_PENDING`. Throws `ERR_DEEP_LINKS_BUSY` for a copy of the link the last wait resolved with.
   */
  deliver(url: string): void;
  /** Fails the waiting `nextUrl` with `code`, else the next call, before any queued link. */
  failNext(code: DeepLinksErrorCode): void;
  /** Fails the waiting `nextUrl` as the platform's bridge would: `ERR_DEEP_LINKS_BACKEND`. */
  failPlatform(): void;
  setWebLinkKind(kind: LinkKind | null): void;
  waiting(): boolean;
  queued(): string[];
  deliveries(): number;
  cancels(): number;
  initialReads(): number;
}

type Waiter = { resolve: (url: string) => void; reject: (error: DeepLinksError) => void };

/** A memory backend, for tests and for apps that run without a platform behind them. */
export function memoryBackend(): MemoryDeepLinks {
  let initial: string | null = null;
  let initialError: DeepLinksErrorCode | null = null;
  let webKind: LinkKind | null = null;
  const queue: string[] = [];
  let waiter: Waiter | null = null;
  let resolving: string | null = null;
  let nextError: DeepLinksError | null = null;
  let deliveries = 0;
  let cancels = 0;
  let initialReads = 0;

  const fail = (error: DeepLinksError) => {
    const waiting = waiter;
    waiter = null;
    if (waiting !== null) waiting.reject(error);
    else nextError = error;
  };

  return {
    initialUrl: async () => {
      initialReads += 1;
      if (initialError !== null) throw deepLinksError(initialError);
      return initial;
    },
    nextUrl: () => {
      resolving = null;
      if (nextError !== null) {
        const error = nextError;
        nextError = null;
        return Promise.reject(error);
      }
      const queued = queue.shift();
      if (queued !== undefined) {
        deliveries += 1;
        resolving = queued;
        return Promise.resolve(queued);
      }
      return new Promise<string>((resolve, reject) => {
        waiter = { resolve, reject };
      });
    },
    cancelWait: () => {
      cancels += 1;
      const waiting = waiter;
      waiter = null;
      waiting?.reject(deepLinksError("ERR_DEEP_LINKS_CANCELLED"));
    },
    webLinkKind: () => webKind,
    setInitial: (url) => {
      initial = url;
    },
    failInitial: (code) => {
      initialError = code;
    },
    deliver: (url) => {
      const waiting = waiter;
      if (waiting !== null) {
        waiter = null;
        deliveries += 1;
        resolving = url;
        waiting.resolve(url);
        return;
      }
      if (resolving === url) {
        throw deepLinksError(
          "ERR_DEEP_LINKS_BUSY",
          "the same link arrived again while the last wait resolved with it",
        );
      }
      if (queue.length >= MAX_PENDING) queue.shift();
      queue.push(url);
    },
    failNext: (code) => fail(deepLinksError(code)),
    failPlatform: () => fail(deepLinksError("ERR_DEEP_LINKS_BACKEND", "the scripted platform failed")),
    setWebLinkKind: (kind) => {
      webKind = kind;
    },
    waiting: () => waiter !== null,
    queued: () => [...queue],
    deliveries: () => deliveries,
    cancels: () => cancels,
    initialReads: () => initialReads,
  };
}
