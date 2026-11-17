import type { LinkKind } from "./links";

/**
 * What a `DeepLinks` runs on: the Rust crate's platform intake, or a memory backend in tests. The
 * mirror of the crate's `Backend` trait: raw URLs only; checking and routing stay in `DeepLinks`.
 */
export interface DeepLinksBackend {
  /** The URL the app was started with, unchecked; `null` when there was none. */
  initialUrl(): Promise<string | null>;
  /** The next URL the system hands the app. Settles exactly once: a URL, or a coded rejection. */
  nextUrl(): Promise<string>;
  /** Rejects the waiting `nextUrl` with `ERR_DEEP_LINKS_CANCELLED`; nothing when none waits. */
  cancelWait(): void;
  /** The kind this platform's verified `http(s)` links are; `null` where it has none. */
  webLinkKind(): LinkKind | null;
}
