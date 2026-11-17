/** The codes a deep-links failure carries. Apps match on `code`; codes are never renamed. */
export type DeepLinksErrorCode =
  | "ERR_DEEP_LINKS_INVALID_URL"
  | "ERR_DEEP_LINKS_URL_TOO_LARGE"
  | "ERR_DEEP_LINKS_DISALLOWED_SCHEME"
  | "ERR_DEEP_LINKS_HOST_NOT_ALLOWED"
  | "ERR_DEEP_LINKS_NO_ROUTE"
  | "ERR_DEEP_LINKS_INVALID_PARAMETER"
  | "ERR_DEEP_LINKS_INVALID_ARGUMENT"
  | "ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM"
  | "ERR_DEEP_LINKS_BUSY"
  | "ERR_DEEP_LINKS_CANCELLED"
  | "ERR_DEEP_LINKS_TIMEOUT"
  | "ERR_DEEP_LINKS_ASSOCIATION_INVALID"
  | "ERR_DEEP_LINKS_BACKEND";

/** Same wording as the Rust crate's `ErrorKind::message`. */
const MESSAGES: Record<DeepLinksErrorCode, string> = {
  ERR_DEEP_LINKS_INVALID_URL: "The link is not an absolute URL a link may be.",
  ERR_DEEP_LINKS_URL_TOO_LARGE: "The link is too long. Links have at most 16 KiB of text.",
  ERR_DEEP_LINKS_DISALLOWED_SCHEME: "The link's scheme is not the app's own and not a verified web link.",
  ERR_DEEP_LINKS_HOST_NOT_ALLOWED: "The link's host is not one the app's prefixes allow.",
  ERR_DEEP_LINKS_NO_ROUTE: "The link's path matches no route in the table.",
  ERR_DEEP_LINKS_INVALID_PARAMETER: "A link parameter is not of the type its route declares.",
  ERR_DEEP_LINKS_INVALID_ARGUMENT: "An argument is empty or of the wrong shape.",
  ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM: "Verified web links are not available on this platform.",
  ERR_DEEP_LINKS_BUSY: "Another link wait is already running. Wait for it to finish.",
  ERR_DEEP_LINKS_CANCELLED: "The link wait was cancelled.",
  ERR_DEEP_LINKS_TIMEOUT: "The platform gave no link before its deadline.",
  ERR_DEEP_LINKS_ASSOCIATION_INVALID: "The association file is malformed, or names the wrong app.",
  ERR_DEEP_LINKS_BACKEND: "The platform's link service failed.",
};

const KNOWN = new Set<string>(Object.keys(MESSAGES));

/** Every code, in the order the goals list them. */
export const DEEP_LINKS_ERROR_CODES = Object.keys(MESSAGES) as DeepLinksErrorCode[];

/** A failed link call or route match. `code` is the stable name; `detail` says what specifically. */
export class DeepLinksError extends Error {
  readonly code: DeepLinksErrorCode;
  readonly detail: string | null;

  constructor(code: DeepLinksErrorCode, detail: string | null = null) {
    super(detail === null ? MESSAGES[code] : `${MESSAGES[code]} ${detail}`);
    this.name = "DeepLinksError";
    this.code = code;
    this.detail = detail;
  }
}

/** The error for a known code, with an optional detail. */
export function deepLinksError(code: DeepLinksErrorCode, detail: string | null = null): DeepLinksError {
  return new DeepLinksError(code, detail);
}

/**
 * Turns whatever the native side or a host threw into a coded `DeepLinksError`. A host's
 * rejection carries `ERR_NSH_HOST_ERROR` with the code leading its message. Anything unknown is a
 * backend failure.
 */
export function asDeepLinksError(error: unknown): DeepLinksError {
  if (error instanceof DeepLinksError) return error;
  const raw = (error ?? {}) as { code?: unknown; message?: unknown };
  const code = typeof raw.code === "string" ? raw.code : "";
  const message = typeof raw.message === "string" ? raw.message : "";
  if (code === "ERR_NSH_HOST_ERROR") {
    const named = /^(ERR_DEEP_LINKS_[A-Z_]+): /.exec(message)?.[1];
    return named !== undefined && KNOWN.has(named)
      ? deepLinksError(named as DeepLinksErrorCode)
      : deepLinksError("ERR_DEEP_LINKS_BACKEND");
  }
  return KNOWN.has(code) ? deepLinksError(code as DeepLinksErrorCode) : deepLinksError("ERR_DEEP_LINKS_BACKEND");
}
