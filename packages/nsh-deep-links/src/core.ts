import { NativeDeepLinks } from "nsh-deep-links/native";

/**
 * NSH core's URL rules, read from the Rust crate (`nsh_core::url_scheme`), never copied: a change
 * in core is a change here.
 */

/** The longest link taken, in bytes (core's `MAX_URL_LEN`). */
export const MAX_URL_LEN: number = NativeDeepLinks.maxUrlLen();

/** The most links held while nothing waits; past it the oldest goes (core's `MAX_PENDING`). */
export const MAX_PENDING: number = NativeDeepLinks.maxPending();

/** Core's `check_url`: `null` when `url` is a link of one of `schemes` (lower case), else why not. */
export function checkUrl(url: string, schemes: string[]): string | null {
  return NativeDeepLinks.checkUrl(url, schemes) ?? null;
}

/** Core's `check_scheme`: `null` when an app may claim `scheme`, else why not. */
export function checkScheme(scheme: string): string | null {
  return NativeDeepLinks.checkScheme(scheme) ?? null;
}
