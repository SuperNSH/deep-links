import { DeepLinksError, deepLinksError } from "./errors";

/**
 * The files that make the web vouch for the app, built and checked offline, as the crate's
 * `association` module. Nothing here fetches: validators take the bytes the caller has.
 */

const HANDLE_ALL_URLS = "delegate_permission/common.handle_all_urls";
const ASSETLINKS_PATH = ".well-known/assetlinks.json";
const APPLE_PATH = ".well-known/apple-app-site-association";

/** One statement file and the address it is served at. */
export interface AssociationFile {
  url: string;
  body: string;
}

function bad(detail: string): DeepLinksError {
  return deepLinksError("ERR_DEEP_LINKS_INVALID_ARGUMENT", detail);
}

function invalid(detail: string): DeepLinksError {
  return deepLinksError("ERR_DEEP_LINKS_ASSOCIATION_INVALID", detail);
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

function androidPackageIsValid(name: string): boolean {
  const words = name.split(".");
  return words.length >= 2 && words.every((word) => /^[A-Za-z][A-Za-z0-9_]*$/.test(word));
}

function appleAppIdIsValid(appId: string): boolean {
  const dot = appId.indexOf(".");
  if (dot === -1) return false;
  const team = appId.slice(0, dot);
  const bundle = appId.slice(dot + 1);
  return /^[A-Z0-9]{10}$/.test(team) && bundle !== "" && bundle.split(".").every((word) => /^[A-Za-z0-9-]+$/.test(word));
}

function fingerprintHex(fingerprint: string): string | null {
  const hex = fingerprint.replaceAll(":", "");
  return /^[0-9A-Fa-f]{64}$/.test(hex) ? hex.toUpperCase() : null;
}

function colonated(hex: string): string {
  return (hex.match(/.{2}/g) ?? []).join(":");
}

function componentIsValid(component: string): boolean {
  return component.startsWith("/") && !/\s/u.test(component);
}

function pretty(value: unknown): string {
  return JSON.stringify(value, null, 2);
}

function parse(text: string): unknown {
  try {
    return JSON.parse(text) as unknown;
  } catch {
    throw invalid("the file is not JSON");
  }
}

function files(hosts: string[], path: string, body: string): AssociationFile[] {
  if (hosts.length === 0) throw bad("at least one host is needed: example.com");
  const out: AssociationFile[] = [];
  for (const host of hosts) {
    const labels = host.split(".");
    if (labels.length < 2 || !labels.every((label) => /^[A-Za-z0-9-]+$/.test(label))) {
      throw bad(`\`${host}\` is not a host that serves a file: \`www.example.com\`, no scheme, port, path or \`*\``);
    }
    const url = `https://${host.toLowerCase()}/${path}`;
    if (out.some((file) => file.url === url)) throw bad(`\`${host}\` is listed twice`);
    out.push({ url, body });
  }
  return out;
}

/** The `assetlinks.json` that vouches for `packageName` with its signing certificates' SHA-256s. */
export function assetlinksJson(packageName: string, fingerprints: string[]): string {
  if (!androidPackageIsValid(packageName)) {
    throw bad(
      `\`${packageName}\` is not an Android package name: two or more dot-separated words, each a letter then letters, digits or \`_\``,
    );
  }
  if (fingerprints.length === 0) throw bad("at least one SHA-256 certificate fingerprint is needed");
  const normalized = fingerprints.map((fingerprint) => {
    const hex = fingerprintHex(fingerprint);
    if (hex === null) throw bad(`\`${fingerprint}\` is not a SHA-256 fingerprint: 64 hex digits, colons allowed`);
    return colonated(hex);
  });
  return pretty([
    {
      relation: [HANDLE_ALL_URLS],
      target: { namespace: "android_app", package_name: packageName, sha256_cert_fingerprints: normalized },
    },
  ]);
}

/** One `assetlinks.json` per host: each subdomain serves its own for `autoVerify`. */
export function assetlinksFiles(hosts: string[], packageName: string, fingerprints: string[]): AssociationFile[] {
  return files(hosts, ASSETLINKS_PATH, assetlinksJson(packageName, fingerprints));
}

/** The `apple-app-site-association` for `appIds` on `components` (none: every path, `/*`). */
export function appleAppSiteAssociation(appIds: string[], components: string[] = []): string {
  if (appIds.length === 0) throw bad("at least one app ID is needed: TEAMID.bundle.id");
  for (const appId of appIds) {
    if (!appleAppIdIsValid(appId)) {
      throw bad(`\`${appId}\` is not an app ID: a 10-character team ID, a dot, the bundle ID`);
    }
  }
  const paths = components.length === 0 ? ["/*"] : components;
  const parsed = paths.map((component) => {
    if (!componentIsValid(component)) {
      throw bad(`\`${component}\` is not a path component: it starts with \`/\` and has no whitespace`);
    }
    return { "/": component };
  });
  return pretty({ applinks: { details: [{ appIDs: appIds, components: parsed }] } });
}

/** One `apple-app-site-association` per host. */
export function appleAppSiteAssociationFiles(
  hosts: string[],
  appIds: string[],
  components: string[] = [],
): AssociationFile[] {
  return files(hosts, APPLE_PATH, appleAppSiteAssociation(appIds, components));
}

/** Checks an `assetlinks.json` for `packageName`; another package alone is `ERR_DEEP_LINKS_ASSOCIATION_INVALID`. */
export function validateAssetlinks(text: string, packageName: string): void {
  if (!androidPackageIsValid(packageName)) throw bad(`\`${packageName}\` is not an Android package name`);
  const file = parse(text);
  if (!Array.isArray(file)) throw invalid('the file is a list of statements: [{ "relation": ..., "target": ... }]');
  if (file.length === 0) throw invalid("the file has no statements");
  let namesIt = false;
  for (const statement of file as unknown[]) {
    if (!isObject(statement)) throw invalid("a statement is an object");
    const relation = statement.relation;
    if (!Array.isArray(relation)) throw invalid('a statement\'s "relation" is an array');
    const target = statement.target;
    if (!isObject(target)) throw invalid('a statement has a "target" object');
    if (target.namespace !== "android_app") continue;
    const named = target.package_name;
    if (typeof named !== "string") throw invalid('an android_app target has a "package_name" string');
    const fingerprints = target.sha256_cert_fingerprints;
    if (!Array.isArray(fingerprints)) throw invalid('an android_app target has a "sha256_cert_fingerprints" array');
    if (fingerprints.length === 0) throw invalid("an android_app target lists at least one fingerprint");
    for (const fingerprint of fingerprints as unknown[]) {
      if (typeof fingerprint !== "string" || fingerprintHex(fingerprint) === null) {
        throw invalid("a fingerprint is 64 hex digits, colon-separated");
      }
    }
    if (named === packageName && (relation as unknown[]).includes(HANDLE_ALL_URLS)) namesIt = true;
  }
  if (!namesIt) throw invalid(`no statement hands \`${packageName}\` the host's URLs (${HANDLE_ALL_URLS})`);
}

/** Checks an `apple-app-site-association` for `appId`; another team or bundle alone is invalid. */
export function validateAppleAppSiteAssociation(text: string, appId: string): void {
  if (!appleAppIdIsValid(appId)) {
    throw bad(`\`${appId}\` is not an app ID: a 10-character team ID, a dot, the bundle ID`);
  }
  const file = parse(text);
  if (!isObject(file)) throw invalid('the file is an object: { "applinks": { "details": [...] } }');
  const applinks = file.applinks;
  const details = isObject(applinks) ? applinks.details : undefined;
  if (!Array.isArray(details)) throw invalid('the file has "applinks": { "details": [...] }');
  let namesIt = false;
  for (const detail of details as unknown[]) {
    if (!isObject(detail)) throw invalid("a detail is an object");
    let listed: string[];
    if (detail.appIDs !== undefined) {
      const ids = detail.appIDs;
      if (!Array.isArray(ids) || ids.length === 0) throw invalid('"appIDs" is a non-empty array');
      const components = detail.components;
      if (!Array.isArray(components)) throw invalid('a detail with "appIDs" has a "components" array');
      for (const component of components as unknown[]) {
        if (!isObject(component)) throw invalid("a component is an object");
        const path = component["/"];
        if (path !== undefined && !(typeof path === "string" && componentIsValid(path))) {
          throw invalid('a component\'s "/" path starts with `/`');
        }
      }
      listed = (ids as unknown[]).map((id) => {
        if (typeof id !== "string") throw invalid("an app ID is a string");
        return id;
      });
    } else if (detail.appID !== undefined) {
      if (typeof detail.appID !== "string") throw invalid('"appID" is a string');
      const paths = detail.paths;
      if (!Array.isArray(paths)) throw invalid('a detail with "appID" has a "paths" array');
      if ((paths as unknown[]).some((p) => typeof p !== "string" || p === "")) {
        throw invalid("a path pattern is a non-empty string");
      }
      listed = [detail.appID];
    } else {
      throw invalid('a detail has "appIDs", or the older "appID"');
    }
    for (const id of listed) {
      if (!appleAppIdIsValid(id)) {
        throw invalid(`\`${id}\` is not an app ID: a 10-character team ID, a dot, the bundle ID`);
      }
      if (id === appId) namesIt = true;
    }
  }
  if (!namesIt) throw invalid(`no detail lists \`${appId}\`: the file is for another team or bundle`);
}
