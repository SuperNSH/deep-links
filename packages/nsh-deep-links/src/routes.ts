import { checkScheme } from "./core";
import { DeepLinksError, deepLinksError } from "./errors";

/** What a `parse` entry turns a param into. */
export type ParamType = "string" | "number" | "integer" | "boolean";

/** A param's value: a string, or what its `parse` type made of it. */
export type ParamValue = string | number | boolean;

/** The screen a link routes to, and its params: path params first, then the query's. */
export interface RouteMatch {
  /** The config key of the matched screen (the leaf, for nested screens). */
  screen: string;
  params: Record<string, ParamValue>;
}

/** A screen's config: a pattern, or an object with one and more. */
export type ScreenConfig =
  | string
  | {
      path?: string;
      parse?: Record<string, ParamType>;
      alias?: string[];
      exact?: boolean;
      screens?: Record<string, ScreenConfig>;
    };

/** A linking config, as React Navigation's `linking` writes it. */
export interface LinkingConfig {
  prefixes?: string[];
  path?: string;
  screens?: Record<string, ScreenConfig>;
}

type Repeat = "one-or-more" | "zero-or-more";

type Part =
  | { kind: "static"; text: string }
  | { kind: "param"; name: string; regex: RegExp | null; optional: boolean; repeat: Repeat | null }
  | { kind: "wildcard" };

interface Route {
  screen: string;
  pattern: string;
  parts: Part[];
  parse: Map<string, ParamType>;
  order: number;
}

type Label = { any: true } | { any: false; text: string };

interface Prefix {
  scheme: string;
  any: boolean;
  labels: Label[];
  path: string;
  raw: string;
}

const TYPES: ParamType[] = ["string", "number", "integer", "boolean"];

function invalid(detail: string): DeepLinksError {
  return deepLinksError("ERR_DEEP_LINKS_INVALID_ARGUMENT", detail);
}

function noRoute(detail: string): DeepLinksError {
  return deepLinksError("ERR_DEEP_LINKS_NO_ROUTE", detail);
}

function isObject(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

/** React Navigation's specificity ranks; a missing segment (the pattern's end) is 5. */
function rank(part: Part | undefined): number {
  if (part === undefined) return 5;
  if (part.kind === "static") return 0;
  if (part.kind === "wildcard") return 8;
  const regex = part.regex !== null;
  if (part.repeat === null) return regex ? 1 : 2;
  if (part.repeat === "one-or-more") return regex ? 3 : 4;
  return regex ? 6 : 7;
}

/** Plain decimal text, as JSON writes a number: no `+`, no hex, no `Infinity`, no spaces. */
function isDecimal(text: string): boolean {
  return /^-?[0-9]+(\.[0-9]+)?([eE][+-]?[0-9]+)?$/.test(text);
}

function parseValue(kind: ParamType, text: string): ParamValue | undefined {
  switch (kind) {
    case "string":
      return text;
    case "boolean":
      return text === "true" ? true : text === "false" ? false : undefined;
    case "integer": {
      if (!/^-?[0-9]+$/.test(text)) return undefined;
      const n = Number(text);
      return Number.isSafeInteger(n) ? (n === 0 ? 0 : n) : undefined;
    }
    case "number": {
      if (!isDecimal(text)) return undefined;
      const n = Number(text);
      if (!Number.isFinite(n)) return undefined;
      return n === 0 ? 0 : n;
    }
  }
}

/** Percent-decodes; `null` for a `%` without two hex digits, or bytes that are not UTF-8. */
function decode(text: string): string | null {
  try {
    return decodeURIComponent(text);
  } catch {
    return null;
  }
}

function isWeb(scheme: string): boolean {
  return scheme === "http" || scheme === "https";
}

function partsPattern(pattern: string): string {
  return pattern
    .split("/")
    .filter((segment) => segment !== "")
    .join("/");
}

function join(parent: string, child: string): string {
  const p = parent.replace(/^\/+|\/+$/g, "");
  const c = child.replace(/^\/+|\/+$/g, "");
  if (p === "") return c;
  if (c === "") return p;
  return `${p}/${c}`;
}

/** Splits a pattern with `getPatternParts`' rules, refusing what it refuses. */
function parsePattern(pattern: string): Part[] {
  const bad = (what: string) => invalid(`the pattern \`${pattern}\`: ${what}`);
  const chars = Array.from(pattern);
  const parts: Part[] = [];
  const names: string[] = [];
  let i = 0;
  while (i < chars.length) {
    if (chars[i] === "/") {
      i += 1;
      continue;
    }
    if (chars[i] !== ":") {
      const start = i;
      while (i < chars.length && chars[i] !== "/") {
        const c = chars[i];
        if (c === ":") throw bad("`:` in the middle of a segment");
        if (c === "(") throw bad("`(` without a param before it");
        if (c === ")") throw bad("`)` without a `(` before it");
        if (c === "?") throw bad("`?` without a param before it");
        i += 1;
      }
      const segment = chars.slice(start, i).join("");
      if (segment === "*") {
        parts.push({ kind: "wildcard" });
      } else {
        const decoded = decode(segment);
        if (decoded === null) throw bad("a percent-escape that does not decode");
        parts.push({ kind: "static", text: decoded });
      }
      continue;
    }
    i += 1;
    const start = i;
    while (i < chars.length && !"/()?+*:".includes(chars[i] ?? "")) i += 1;
    const name = chars.slice(start, i).join("");
    if (name === "") throw bad("a param has a name");
    if (names.includes(name)) throw bad(`the param \`:${name}\` is named twice`);
    let regex: RegExp | null = null;
    if (chars[i] === "(") {
      const found = regexSource(chars, i);
      if (found === null) throw bad("an unclosed `(`");
      try {
        regex = new RegExp(`^(?:${found.source})$`, "u");
      } catch {
        throw bad(`\`${found.source}\` is not a regex`);
      }
      i = found.end;
    }
    let optional = false;
    let repeat: Repeat | null = null;
    let modifiers = 0;
    while (i < chars.length && chars[i] !== "/") {
      const c = chars[i];
      if (c === "?") optional = true;
      else if (c === "+") repeat = "one-or-more";
      else if (c === "*") repeat = "zero-or-more";
      else if (c === ":") throw bad("`:` in the middle of a segment");
      else throw bad("text after a param's modifier");
      modifiers += 1;
      i += 1;
    }
    if (modifiers > 1) throw bad("param modifiers do not combine");
    names.push(name);
    parts.push({ kind: "param", name, regex, optional, repeat });
  }
  return parts;
}

/** The regex between the `(` at `open` and its matching `)`, escapes and classes respected. */
function regexSource(chars: string[], open: number): { source: string; end: number } | null {
  let depth = 0;
  let escaped = false;
  let inClass = false;
  let source = "";
  for (let at = open + 1; at < chars.length; at += 1) {
    const c = chars[at] ?? "";
    if (escaped) escaped = false;
    else if (c === "\\") escaped = true;
    else if (inClass) inClass = c !== "]";
    else if (c === "[") inClass = true;
    else if (c === "(") depth += 1;
    else if (c === ")") {
      if (depth === 0) return { source, end: at + 1 };
      depth -= 1;
    }
    source += c;
  }
  return null;
}

/** Whether `parts` match all of `segments`; repeats and the wildcard take the most they can. */
function walk(parts: Part[], segments: string[], params: Map<string, string>): boolean {
  const part = parts[0];
  if (part === undefined) return segments.length === 0;
  const rest = parts.slice(1);
  if (part.kind === "static") {
    return segments[0] === part.text && walk(rest, segments.slice(1), params);
  }
  if (part.kind === "wildcard") {
    for (let taken = segments.length; taken >= 0; taken -= 1) {
      if (walk(rest, segments.slice(taken), params)) return true;
    }
    return false;
  }
  const regex = part.regex;
  const fits = (segment: string) => regex === null || regex.test(segment);
  let least: number;
  let most: number;
  if (part.repeat === null) {
    least = part.optional ? 0 : 1;
    most = 1;
  } else {
    least = part.repeat === "one-or-more" ? 1 : 0;
    most = segments.length;
  }
  for (let taken = Math.min(most, segments.length); taken >= least; taken -= 1) {
    const used = segments.slice(0, taken);
    if (!used.every(fits)) continue;
    const absent = taken === 0 && part.repeat === null;
    if (!absent) params.set(part.name, used.join("/"));
    if (walk(rest, segments.slice(taken), params)) return true;
    params.delete(part.name);
  }
  return false;
}

/** The query's pairs, decoded (raw where they do not decode); a later key replaces an earlier one. */
function parseQuery(query: string): [string, string][] {
  const pairs: [string, string][] = [];
  for (const pair of query.split("&")) {
    if (pair === "") continue;
    const at = pair.indexOf("=");
    const rawKey = at === -1 ? pair : pair.slice(0, at);
    const rawValue = at === -1 ? "" : pair.slice(at + 1);
    const key = decode(rawKey) ?? rawKey;
    const value = decode(rawValue) ?? rawValue;
    const existing = pairs.findIndex(([k]) => k === key);
    if (existing !== -1) pairs.splice(existing, 1);
    pairs.push([key, value]);
  }
  return pairs;
}

function splitPrefix(prefix: string): Prefix {
  if (prefix === "*") return { scheme: "", any: true, labels: [], path: "", raw: prefix };
  const colon = prefix.indexOf(":");
  if (colon === -1) {
    throw invalid(`\`${prefix}\` is not a prefix: "scheme://host", "scheme://" or "*"`);
  }
  const scheme = prefix.slice(0, colon).toLowerCase();
  if (!isWeb(scheme)) {
    const reason = checkScheme(scheme);
    if (reason !== null) {
      // A well-formed name fails core's check only for being reserved, and appending a letter
      // keeps a name well-formed: that tells the two refusals apart.
      const reserved = scheme !== "" && checkScheme(`${scheme}x`) === null;
      throw deepLinksError(reserved ? "ERR_DEEP_LINKS_DISALLOWED_SCHEME" : "ERR_DEEP_LINKS_INVALID_ARGUMENT", reason);
    }
  }
  let rest = prefix.slice(colon + 1);
  if (/[?#]/.test(rest) || /\s/u.test(rest)) {
    throw invalid(`\`${prefix}\`: a prefix is a scheme, a host and a path, with no query or fragment`);
  }
  rest = rest.replace(/^\/+/, "");
  const slash = rest.indexOf("/");
  const host = slash === -1 ? rest : rest.slice(0, slash);
  const path = slash === -1 ? "" : partsPattern(rest.slice(slash));
  if (isWeb(scheme) && host === "") throw invalid(`\`${prefix}\`: an http(s) prefix names its host`);
  const labels: Label[] = [];
  if (host !== "") {
    for (const label of host.split(".")) {
      if (label === "*") labels.push({ any: true });
      else if (label === "") throw invalid(`\`${prefix}\`: a host label is not empty`);
      else labels.push({ any: false, text: label });
    }
  }
  return { scheme, any: false, labels, path: path === "" ? "" : `/${path}`, raw: prefix };
}

/** Matches `labels` against all of `host`; the characters used, or -1. */
function matchLabels(labels: Label[], host: string): number {
  const first = labels[0];
  if (first === undefined) return -1;
  const rest = labels.slice(1);
  if (!first.any) {
    const head = host.slice(0, first.text.length);
    if (head.length !== first.text.length || head.toLowerCase() !== first.text.toLowerCase()) return -1;
    if (rest.length === 0) return first.text.length;
    if (host[first.text.length] !== ".") return -1;
    const more = matchLabels(rest, host.slice(first.text.length + 1));
    return more === -1 ? -1 : first.text.length + 1 + more;
  }
  if (rest.length === 0) return host === "" ? -1 : host.length;
  for (let at = host.lastIndexOf("."); at > 0; at = host.lastIndexOf(".", at - 1)) {
    const more = matchLabels(rest, host.slice(at + 1));
    if (more !== -1) return at + 1 + more;
  }
  return -1;
}

/** How many characters of `head` the prefix takes; -1 when it does not match. */
function stripPrefix(prefix: Prefix, head: string): number {
  const colon = head.indexOf(":");
  if (colon <= 0) return -1;
  const scheme = head.slice(0, colon);
  if (scheme.includes("/")) return -1;
  let at = colon + 1;
  while (head[at] === "/") at += 1;
  if (prefix.any) {
    if (isWeb(scheme.toLowerCase())) {
      const rest = head.slice(at);
      const end = rest.indexOf("/") === -1 ? rest.length : rest.indexOf("/");
      if (end === 0) return -1;
      at += end;
    }
    return at;
  }
  if (scheme.toLowerCase() !== prefix.scheme) return -1;
  if (prefix.labels.length > 0) {
    const rest = head.slice(at);
    const end = rest.indexOf("/") === -1 ? rest.length : rest.indexOf("/");
    if (matchLabels(prefix.labels, rest.slice(0, end)) !== end) return -1;
    at += end;
  }
  if (prefix.path !== "") {
    const rest = head.slice(at);
    if (!rest.startsWith(prefix.path)) return -1;
    const after = rest.slice(prefix.path.length);
    if (!(after === "" || after.startsWith("/"))) return -1;
    at += prefix.path.length;
  }
  return at;
}

function routeHasPathParam(route: Route, name: string): boolean {
  return route.parts.some((part) => part.kind === "param" && part.name === name);
}

function typed(route: Route, name: string, text: string): ParamValue {
  const kind = route.parse.get(name);
  if (kind === undefined) return text;
  const value = parseValue(kind, text);
  if (value === undefined) {
    throw deepLinksError(
      "ERR_DEEP_LINKS_INVALID_PARAMETER",
      `the param \`${name}\` of \`${route.screen}\` is not a ${kind}`,
    );
  }
  return value;
}

function collectScreens(screens: Record<string, unknown>, parent: string, routes: Route[]): void {
  // Sorted, so equally specific patterns tie-break the same as in Rust.
  const names = Object.keys(screens).sort((a, b) => (a < b ? -1 : a > b ? 1 : 0));
  for (const screen of names) {
    if (screen === "") throw invalid("a screen's name is not empty");
    const spec = screens[screen];
    const patterns: string[] = [];
    const parse = new Map<string, ParamType>();
    let ownPath: string | null = null;
    let exact = false;
    let children: Record<string, unknown> | null = null;
    if (typeof spec === "string") {
      ownPath = spec;
    } else if (isObject(spec)) {
      for (const key of Object.keys(spec)) {
        if (!["path", "parse", "alias", "exact", "screens"].includes(key)) {
          throw invalid(
            `\`${key}\` is not a key of a screen's config (path, parse, alias, exact, screens): screens.${screen}`,
          );
        }
      }
      if (spec.path !== undefined) {
        if (typeof spec.path !== "string") throw invalid(`screens.${screen}: \`path\` is a string`);
        ownPath = spec.path;
      }
      if (spec.exact !== undefined) {
        if (typeof spec.exact !== "boolean") throw invalid(`screens.${screen}: \`exact\` is a boolean`);
        exact = spec.exact;
      }
      if (spec.parse !== undefined) {
        if (!isObject(spec.parse)) throw invalid(`screens.${screen}: \`parse\` is an object: { "id": "integer" }`);
        for (const [name, kind] of Object.entries(spec.parse)) {
          if (typeof kind !== "string" || !TYPES.includes(kind as ParamType)) {
            throw invalid(`screens.${screen}: parse.${name} is "string", "number", "integer" or "boolean"`);
          }
          parse.set(name, kind as ParamType);
        }
      }
      if (spec.alias !== undefined) {
        if (!Array.isArray(spec.alias)) throw invalid(`screens.${screen}: \`alias\` is an array of patterns`);
        if (ownPath === null && spec.alias.length > 0) {
          throw invalid(`screens.${screen} has an \`alias\` but no \`path\`; an alias is another path`);
        }
        for (const alias of spec.alias as unknown[]) {
          if (typeof alias !== "string") throw invalid(`screens.${screen}: an alias is a pattern string`);
          patterns.push(alias);
        }
      }
      if (spec.screens !== undefined) {
        if (!isObject(spec.screens)) throw invalid(`screens.${screen}: \`screens\` is an object`);
        children = spec.screens;
      }
    } else {
      throw invalid(`screens.${screen} is a pattern string, or an object: { "path": "orders/:id" }`);
    }
    const base = exact ? "" : parent;
    if (ownPath !== null) patterns.unshift(ownPath);
    for (const written of patterns) {
      const pattern = join(base, written);
      const parts = parsePattern(pattern);
      const normalized = partsPattern(pattern);
      if (routes.some((r) => r.screen === screen && r.pattern === normalized)) continue;
      routes.push({ screen, pattern: normalized, parts, parse: new Map(parse), order: routes.length });
    }
    if (children !== null) {
      collectScreens(children, ownPath !== null ? join(base, ownPath) : base, routes);
    }
  }
}

/** An app's validated linking config: prefixes, and the patterns that map paths to screens. */
export class RouteTable {
  private constructor(
    private readonly prefixList: Prefix[],
    private readonly routes: Route[],
  ) {}

  /** A table with no prefixes and no routes: every link is without a route. */
  static empty(): RouteTable {
    return new RouteTable([], []);
  }

  /**
   * Builds the table from a config. A bad pattern, an extraneous key, two screens on one pattern
   * or params in the root `path` throw `ERR_DEEP_LINKS_INVALID_ARGUMENT`; a prefix claiming a
   * reserved scheme throws `ERR_DEEP_LINKS_DISALLOWED_SCHEME`.
   */
  static fromConfig(config: LinkingConfig | unknown): RouteTable {
    if (!isObject(config)) throw invalid('the config is an object: { "prefixes": [...], "screens": { ... } }');
    for (const key of Object.keys(config)) {
      if (!["prefixes", "path", "screens"].includes(key)) {
        throw invalid(`\`${key}\` is not a key of a linking config (prefixes, path, screens)`);
      }
    }
    const prefixes: Prefix[] = [];
    if (config.prefixes !== undefined) {
      if (!Array.isArray(config.prefixes)) throw invalid('"prefixes" is an array: ["https://example.com", "myapp://"]');
      for (const prefix of config.prefixes as unknown[]) {
        if (typeof prefix !== "string") throw invalid('a prefix is a string: "https://example.com"');
        prefixes.push(splitPrefix(prefix));
      }
    }
    let root = "";
    if (config.path !== undefined) {
      if (typeof config.path !== "string") throw invalid('the root "path" is a string');
      root = config.path;
    }
    if (parsePattern(root).some((part) => part.kind !== "static")) {
      throw invalid(`the root path \`${root}\` has params; params belong to screens`);
    }
    const routes: Route[] = [];
    if (config.screens !== undefined) {
      if (!isObject(config.screens)) throw invalid('"screens" is an object: { "Orders": "orders/:id" }');
      collectScreens(config.screens, root, routes);
    }
    routes.forEach((route, i) => {
      const other = routes.slice(0, i).find((o) => o.pattern === route.pattern && o.screen !== route.screen);
      if (other !== undefined) {
        throw invalid(
          `the pattern \`${route.pattern}\` is claimed by \`${other.screen}\` and \`${route.screen}\`; a pattern names one screen`,
        );
      }
    });
    routes.sort((a, b) => {
      const length = Math.max(a.parts.length, b.parts.length);
      for (let i = 0; i < length; i += 1) {
        const difference = rank(a.parts[i]) - rank(b.parts[i]);
        if (difference !== 0) return difference;
      }
      return a.order - b.order;
    });
    return new RouteTable(prefixes, routes);
  }

  /** The prefixes as configured. */
  prefixes(): string[] {
    return this.prefixList.map((prefix) => prefix.raw);
  }

  /** The custom schemes the prefixes claim, lower case, each once. */
  claimedSchemes(): string[] {
    const schemes: string[] = [];
    for (const prefix of this.prefixList) {
      if (prefix.any || isWeb(prefix.scheme) || schemes.includes(prefix.scheme)) continue;
      schemes.push(prefix.scheme);
    }
    return schemes;
  }

  /** Whether an `http(s)` link's authority (`host` or `host:port`) is one a prefix of the same scheme names. */
  allowsHost(scheme: string, authority: string): boolean {
    return this.prefixList.some(
      (prefix) =>
        !prefix.any &&
        prefix.scheme === scheme.toLowerCase() &&
        isWeb(prefix.scheme) &&
        matchLabels(prefix.labels, authority) === authority.length,
    );
  }

  /**
   * The route `url` maps to; the fragment is not part of the match. Throws
   * `ERR_DEEP_LINKS_NO_ROUTE` or `ERR_DEEP_LINKS_INVALID_PARAMETER`.
   */
  matchUrl(url: string): RouteMatch {
    const hash = url.indexOf("#");
    const withoutFragment = hash === -1 ? url : url.slice(0, hash);
    const mark = withoutFragment.indexOf("?");
    const head = mark === -1 ? withoutFragment : withoutFragment.slice(0, mark);
    const query = mark === -1 ? null : withoutFragment.slice(mark + 1);
    for (const prefix of this.prefixList) {
      const used = stripPrefix(prefix, head);
      if (used !== -1) return this.matchParts(head.slice(used), query);
    }
    throw noRoute("no prefix of the route table matches the link");
  }

  /** The route a path (`/orders/42?tab=lines`) maps to, without a prefix. */
  matchPath(path: string): RouteMatch {
    const hash = path.indexOf("#");
    const withoutFragment = hash === -1 ? path : path.slice(0, hash);
    const mark = withoutFragment.indexOf("?");
    return mark === -1
      ? this.matchParts(withoutFragment, null)
      : this.matchParts(withoutFragment.slice(0, mark), withoutFragment.slice(mark + 1));
  }

  private matchParts(path: string, query: string | null): RouteMatch {
    const segments: string[] = [];
    for (const raw of path.split("/")) {
      if (raw === "") continue;
      const decoded = decode(raw);
      if (decoded === null) throw noRoute("a path segment has a percent-escape that does not decode");
      segments.push(decoded);
    }
    for (const route of this.routes) {
      const found = new Map<string, string>();
      if (!walk(route.parts, segments, found)) continue;
      const params: Record<string, ParamValue> = {};
      const names = [...found.keys()].sort();
      for (const name of names) params[name] = typed(route, name, found.get(name) ?? "");
      for (const [name, text] of parseQuery(query ?? "")) {
        if (routeHasPathParam(route, name)) continue;
        params[name] = typed(route, name, text);
      }
      return { screen: route.screen, params };
    }
    throw noRoute("no pattern of the route table matches the path");
  }
}
