import type { DeepLinksBackend } from "./backend";
import { MAX_URL_LEN, checkUrl } from "./core";
import { DeepLinksError, asDeepLinksError, deepLinksError } from "./errors";
import type { RouteMatch, RouteTable } from "./routes";
import { rustBackend } from "./rust";

/**
 * How a link reached the app: a Universal Link (Apple), an App Link (Android), a custom scheme the
 * app claims, or the web page's own address. Never downgraded to make a link match.
 */
export type LinkKind = "universal" | "app" | "custom" | "web";

/** One link the system handed the app: checked, classified and routed. */
export interface Link {
  /** The URL as it came in, fragment included. */
  url: string;
  kind: LinkKind;
  /** The screen it routes to. */
  route: RouteMatch;
}

interface Shared {
  backend: DeepLinksBackend;
  routes: RouteTable;
  busy: boolean;
  stopped: boolean;
  initial: Promise<Link | null> | null;
  held: Link | null;
}

/**
 * Deep links for one app, after `app_links`: `initial` is the link that started it, `next` the
 * next one to arrive, `stop` ends delivery. One wait runs at a time; every call settles once.
 */
export class DeepLinks {
  private readonly shared: Shared;

  constructor(backend: DeepLinksBackend, routes: RouteTable) {
    this.shared = { backend, routes, busy: false, stopped: false, initial: null, held: null };
  }

  /** Deep links through the Rust crate's platform intake. */
  static platform(routes: RouteTable): DeepLinks {
    return new DeepLinks(rustBackend(), routes);
  }

  /** The route table links route through. */
  routes(): RouteTable {
    return this.shared.routes;
  }

  /**
   * The link that started the app: read from the platform once, the same answer on every later
   * call; `null` when no link started it. A launch link that fails the checks is that failure.
   */
  initial(): Promise<Link | null> {
    if (this.shared.initial === null) {
      this.shared.initial = (async () => {
        let raw: string | null;
        try {
          raw = await this.shared.backend.initialUrl();
        } catch (error) {
          throw asDeepLinksError(error);
        }
        return raw === null ? null : resolveLink(this.shared, raw);
      })();
    }
    return this.shared.initial;
  }

  /**
   * Waits for the next link, then checks, classifies and routes it; a link that fails is the coded
   * rejection itself. A second call while one waits is `ERR_DEEP_LINKS_BUSY`; `stop` rejects the
   * wait with `ERR_DEEP_LINKS_CANCELLED`.
   */
  next(): Promise<Link> {
    return this.wait(null);
  }

  /** `next`, rejecting with `ERR_DEEP_LINKS_TIMEOUT` once `timeoutMs` pass with no link. */
  nextWithin(timeoutMs: number): Promise<Link> {
    if (!(timeoutMs > 0)) {
      return Promise.reject(deepLinksError("ERR_DEEP_LINKS_INVALID_ARGUMENT", "a deadline is longer than zero"));
    }
    return this.wait(timeoutMs);
  }

  private wait(timeoutMs: number | null): Promise<Link> {
    const shared = this.shared;
    if (shared.stopped) return Promise.reject(deepLinksError("ERR_DEEP_LINKS_CANCELLED", "delivery was stopped"));
    if (shared.busy) return Promise.reject(deepLinksError("ERR_DEEP_LINKS_BUSY"));
    shared.busy = true;
    return new Promise<Link>((resolve, reject) => {
      let settled = false;
      let timer: ReturnType<typeof setTimeout> | null = null;
      const settle = (answer: { ok: true; url: string } | { ok: false; error: DeepLinksError }) => {
        if (settled) return;
        settled = true;
        if (timer !== null) clearTimeout(timer);
        shared.busy = false;
        if (!answer.ok) {
          reject(answer.error);
          return;
        }
        try {
          resolve(resolveLink(shared, answer.url));
        } catch (error) {
          reject(asDeepLinksError(error));
        }
      };
      if (timeoutMs !== null) {
        timer = setTimeout(() => {
          settle({ ok: false, error: deepLinksError("ERR_DEEP_LINKS_TIMEOUT") });
          shared.backend.cancelWait();
        }, timeoutMs);
      }
      shared.backend.nextUrl().then(
        (url) => settle({ ok: true, url }),
        (error: unknown) => settle({ ok: false, error: asDeepLinksError(error) }),
      );
    });
  }

  /**
   * Ends delivery: the waiting `next` rejects with `ERR_DEEP_LINKS_CANCELLED`, and so does every
   * later one. Resolves `true` when a wait was cancelled.
   */
  async stop(): Promise<boolean> {
    const busy = this.shared.busy;
    this.shared.stopped = true;
    if (busy) this.shared.backend.cancelWait();
    return busy;
  }

  /** The pending-link slot: one link held while the app cannot route it yet. */
  pending(): PendingLink {
    return new PendingLink(this.shared);
  }

  /** Runs `url` through the checks, classification and routing of a delivered link, now. */
  resolve(url: string): Link {
    return resolveLink(this.shared, url);
  }
}

/** One held link, held while the app cannot route it (before a login, say) and claimed after. */
export class PendingLink {
  /** @internal */
  constructor(private readonly shared: Shared) {}

  /** Holds `link`; a newer link replaces the held one. */
  hold(link: Link): void {
    this.shared.held = link;
  }

  /** Takes the held link: handed out once, and the slot is empty after. */
  claim(): Link | null {
    const held = this.shared.held;
    this.shared.held = null;
    return held;
  }

  /** Empties the slot. */
  clear(): void {
    this.shared.held = null;
  }

  /** The held link, left in the slot. */
  held(): Link | null {
    return this.shared.held;
  }
}

function utf8Length(text: string): number {
  let bytes = 0;
  for (const char of text) {
    const code = char.codePointAt(0) ?? 0;
    bytes += code < 0x80 ? 1 : code < 0x800 ? 2 : code < 0x10000 ? 3 : 4;
  }
  return bytes;
}

function parsed(raw: string): URL | null {
  try {
    return new URL(raw);
  } catch {
    return null;
  }
}

/** The pipeline every link goes through, as in the crate: core's `check_url`, then the host
 * allowlist and the platform's kind for verified links, then the route. */
function resolveLink(shared: Shared, raw: string): Link {
  const routes = shared.routes;
  let kind: LinkKind;
  if (checkUrl(raw, ["http", "https"]) === null) {
    const url = parsed(raw);
    if (url === null || url.hostname === "") throw deepLinksError("ERR_DEEP_LINKS_INVALID_URL");
    const authority = url.port === "" ? url.hostname : `${url.hostname}:${url.port}`;
    const scheme = url.protocol.replace(/:$/, "");
    if (!routes.allowsHost(scheme, authority)) throw deepLinksError("ERR_DEEP_LINKS_HOST_NOT_ALLOWED");
    const web = shared.backend.webLinkKind();
    if (web === null) {
      throw deepLinksError("ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM", "this platform delivers no verified web links");
    }
    kind = web;
  } else if (checkUrl(raw, routes.claimedSchemes()) === null) {
    kind = "custom";
  } else {
    throw refusal(raw);
  }
  return { url: raw, kind, route: routes.matchUrl(raw) };
}

/** The code for a URL core's `check_url` refused for every scheme the app takes. */
function refusal(raw: string): DeepLinksError {
  if (utf8Length(raw) > MAX_URL_LEN) return deepLinksError("ERR_DEEP_LINKS_URL_TOO_LARGE");
  if (/[\s\p{Cc}]/u.test(raw)) {
    return deepLinksError("ERR_DEEP_LINKS_INVALID_URL", "a link has no whitespace or control characters");
  }
  if (parsed(raw) === null) return deepLinksError("ERR_DEEP_LINKS_INVALID_URL", "a link is an absolute URL");
  return deepLinksError("ERR_DEEP_LINKS_DISALLOWED_SCHEME");
}
