import type { DeepLinksBackend } from "./backend";
import { deepLinksError } from "./errors";

/**
 * The backend of a platform that delivers no links, the mirror of the crate's
 * `platform::unsupported::UnsupportedBackend`: no launch link, every wait rejected at once with
 * `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM`, no verified-link kind.
 */
export function unsupportedBackend(): DeepLinksBackend {
  return {
    initialUrl: async () => null,
    nextUrl: () =>
      Promise.reject(deepLinksError("ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM", "this platform delivers no links")),
    cancelWait: () => {},
    webLinkKind: () => null,
  };
}
