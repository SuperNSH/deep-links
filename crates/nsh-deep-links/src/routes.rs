//! The route table: prefixes and patterns that map a link to a screen, ported from React
//! Navigation's linking config (`getPatternParts`, `getStateFromPath`, `validatePathConfig`).
//!
//! Patterns take `:param`, `:param?`, `:param(regex)`, `:param+`, `:param*` and `*`; screens nest
//! through `screens`, with `alias` patterns and `exact`; `parse` types a param. Unlike React
//! Navigation, a bad config fails [`RouteTable::from_config`] instead of the first link, scheme and
//! host match case-insensitively, and the result is one flat [`RouteMatch`], not a nested state.

use std::collections::BTreeMap;

use nsh_core::JsonValue;
use regex::Regex;
use serde_json::{Map, json};

use crate::error::{DeepLinksError, ErrorKind};

/// What a `parse` entry turns a param into. The config writes the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum ParamType {
    /// `"string"`: any text, as a JSON string.
    String,
    /// `"number"`: a finite decimal, as a JSON number.
    Number,
    /// `"integer"`: a whole decimal, as a JSON number.
    Integer,
    /// `"boolean"`: `true` or `false`, as a JSON boolean.
    Boolean,
}

impl ParamType {
    /// The name the config writes.
    pub fn as_str(self) -> &'static str {
        match self {
            ParamType::String => "string",
            ParamType::Number => "number",
            ParamType::Integer => "integer",
            ParamType::Boolean => "boolean",
        }
    }

    /// The type a name names; `None` for any other.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "string" => Some(ParamType::String),
            "number" => Some(ParamType::Number),
            "integer" => Some(ParamType::Integer),
            "boolean" => Some(ParamType::Boolean),
            _ => None,
        }
    }

    /// The JSON value `text` is of this type; `None` when it is not one.
    fn parse(self, text: &str) -> Option<JsonValue> {
        match self {
            ParamType::String => Some(json!(text)),
            ParamType::Boolean => match text {
                "true" => Some(json!(true)),
                "false" => Some(json!(false)),
                _ => None,
            },
            ParamType::Integer => {
                if !is_decimal(text) || text.contains(['.', 'e', 'E']) {
                    return None;
                }
                // Whole numbers a JS number holds exactly, so both languages take the same ones.
                text.parse::<i64>()
                    .ok()
                    .filter(|n| n.unsigned_abs() < 1 << 53)
                    .map(|n| json!(n))
            }
            ParamType::Number => {
                if !is_decimal(text) {
                    return None;
                }
                let number: f64 = text.parse().ok()?;
                if !number.is_finite() {
                    return None;
                }
                if number.fract() == 0.0 && number.abs() < 9_007_199_254_740_992.0 {
                    // A whole number below 2^53 is one in both languages: an integer in JSON.
                    Some(json!(number as i64))
                } else {
                    Some(json!(number))
                }
            }
        }
    }
}

/// Plain decimal text, as JSON writes a number: no `+`, no hex, no `Infinity`, no spaces.
fn is_decimal(text: &str) -> bool {
    let body = text.strip_prefix('-').unwrap_or(text);
    let (mantissa, exponent) = match body.find(['e', 'E']) {
        Some(at) => (&body[..at], Some(&body[at + 1..])),
        None => (body, None),
    };
    let (whole, fraction) = match mantissa.split_once('.') {
        Some((whole, fraction)) => (whole, Some(fraction)),
        None => (mantissa, None),
    };
    let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    digits(whole)
        && fraction.is_none_or(digits)
        && exponent.is_none_or(|e| digits(e.strip_prefix(['+', '-']).unwrap_or(e)))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Repeat {
    OneOrMore,
    ZeroOrMore,
}

/// One segment of a pattern.
#[derive(Debug, Clone)]
enum Part {
    /// Literal text, percent-decoded.
    Static(String),
    Param {
        name: String,
        regex: Option<Regex>,
        optional: bool,
        repeat: Option<Repeat>,
    },
    /// `*`: any number of segments.
    Wildcard,
}

impl Part {
    /// React Navigation's specificity ranks; a missing segment (the pattern's end) is 5.
    fn rank(&self) -> u8 {
        match self {
            Part::Static(_) => 0,
            Part::Param { regex, repeat, .. } => match (repeat, regex.is_some()) {
                (None, true) => 1,
                (None, false) => 2,
                (Some(Repeat::OneOrMore), true) => 3,
                (Some(Repeat::OneOrMore), false) => 4,
                (Some(Repeat::ZeroOrMore), true) => 6,
                (Some(Repeat::ZeroOrMore), false) => 7,
            },
            Part::Wildcard => 8,
        }
    }
}

#[derive(Debug, Clone)]
struct Route {
    screen: String,
    /// The full pattern, normalized: segments joined by one `/`.
    pattern: String,
    parts: Vec<Part>,
    parse: BTreeMap<String, ParamType>,
    order: usize,
}

/// A host label of a prefix: literal text, or `*` for a run of whole labels.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Label {
    Any,
    Lit(String),
}

#[derive(Debug, Clone)]
struct Prefix {
    /// Lower case. Empty for `*`.
    scheme: String,
    any: bool,
    /// Host labels; empty when the prefix names no host (`myapp://`).
    labels: Vec<Label>,
    /// After the host: empty, or `/` and what follows, without a trailing `/`.
    path: String,
    raw: String,
}

/// The screen a link routes to, and its params: path params first, then the query's.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteMatch {
    /// The config key of the matched screen (the leaf, for nested screens).
    pub screen: String,
    /// Decoded params; typed by the route's `parse`, else strings.
    pub params: BTreeMap<String, JsonValue>,
}

/// An app's validated linking config: prefixes, and the patterns that map paths to screens.
#[derive(Debug, Clone, Default)]
pub struct RouteTable {
    prefixes: Vec<Prefix>,
    routes: Vec<Route>,
}

fn invalid(detail: impl Into<String>) -> DeepLinksError {
    DeepLinksError::new(ErrorKind::InvalidArgument).with_detail(detail)
}

fn no_route(detail: &str) -> DeepLinksError {
    DeepLinksError::new(ErrorKind::NoRoute).with_detail(detail)
}

impl RouteTable {
    /// A table with no prefixes and no routes: every link is without a route.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the table from a config, as React Navigation's `linking` writes it:
    ///
    /// ```json
    /// {
    ///   "prefixes": ["https://example.com", "myapp://"],
    ///   "path": "app",
    ///   "screens": {
    ///     "Orders": "orders/:id",
    ///     "Chat": { "path": "chat/:author/:id(\\d+)", "parse": { "id": "integer" } },
    ///     "Settings": { "path": "settings", "screens": { "Profile": "profile" } }
    ///   }
    /// }
    /// ```
    ///
    /// A bad pattern, an extraneous key, two screens on one pattern or params in the root `path`
    /// fail with `ERR_DEEP_LINKS_INVALID_ARGUMENT`; a prefix claiming a reserved scheme
    /// (`mailto:`, `file:`, ...) fails with `ERR_DEEP_LINKS_DISALLOWED_SCHEME`.
    pub fn from_config(value: &JsonValue) -> Result<Self, DeepLinksError> {
        let object = value.as_object().ok_or_else(|| {
            invalid("the config is an object: { \"prefixes\": [...], \"screens\": { ... } }")
        })?;
        for key in object.keys() {
            if !matches!(key.as_str(), "prefixes" | "path" | "screens") {
                return Err(invalid(format!(
                    "`{key}` is not a key of a linking config (prefixes, path, screens)"
                )));
            }
        }
        let mut prefixes = Vec::new();
        if let Some(value) = object.get("prefixes") {
            let list = value.as_array().ok_or_else(|| {
                invalid("\"prefixes\" is an array: [\"https://example.com\", \"myapp://\"]")
            })?;
            for prefix in list {
                let prefix = prefix
                    .as_str()
                    .ok_or_else(|| invalid("a prefix is a string: \"https://example.com\""))?;
                prefixes.push(split_prefix(prefix)?);
            }
        }
        let root = match object.get("path") {
            None => String::new(),
            Some(JsonValue::String(path)) => path.clone(),
            Some(_) => return Err(invalid("the root \"path\" is a string")),
        };
        let root_parts = parse_pattern(&root)?;
        if root_parts
            .iter()
            .any(|part| !matches!(part, Part::Static(_)))
        {
            return Err(invalid(format!(
                "the root path `{root}` has params; params belong to screens"
            )));
        }

        let mut routes = Vec::new();
        if let Some(screens) = object.get("screens") {
            let screens = screens.as_object().ok_or_else(|| {
                invalid("\"screens\" is an object: { \"Orders\": \"orders/:id\" }")
            })?;
            collect_screens(screens, &root, &mut routes)?;
        }
        for (i, route) in routes.iter().enumerate() {
            if let Some(other) = routes[..i]
                .iter()
                .find(|other| other.pattern == route.pattern && other.screen != route.screen)
            {
                return Err(invalid(format!(
                    "the pattern `{}` is claimed by `{}` and `{}`; a pattern names one screen",
                    route.pattern, other.screen, route.screen
                )));
            }
        }
        routes.sort_by(|a, b| {
            let len = a.parts.len().max(b.parts.len());
            for i in 0..len {
                let left = a.parts.get(i).map_or(5, Part::rank);
                let right = b.parts.get(i).map_or(5, Part::rank);
                if left != right {
                    return left.cmp(&right);
                }
            }
            a.order.cmp(&b.order)
        });
        Ok(Self { prefixes, routes })
    }

    /// The prefixes as configured.
    pub fn prefixes(&self) -> Vec<String> {
        self.prefixes.iter().map(|p| p.raw.clone()).collect()
    }

    /// The custom schemes the prefixes claim, lower case, each once: what NSH core's
    /// `check_url` takes a link of. `http(s)` prefixes are verified web links, not claims.
    pub fn claimed_schemes(&self) -> Vec<String> {
        let mut schemes: Vec<String> = Vec::new();
        for prefix in &self.prefixes {
            if prefix.any || is_web(&prefix.scheme) || schemes.contains(&prefix.scheme) {
                continue;
            }
            schemes.push(prefix.scheme.clone());
        }
        schemes
    }

    /// Whether an `http(s)` link's authority (`host` or `host:port`) is one an `http(s)` prefix
    /// of the same scheme names, compared case-insensitively; a `*` label stands for whole labels.
    pub fn allows_host(&self, scheme: &str, authority: &str) -> bool {
        self.prefixes.iter().any(|prefix| {
            !prefix.any
                && prefix.scheme.eq_ignore_ascii_case(scheme)
                && is_web(&prefix.scheme)
                && match_labels(&prefix.labels, authority) == Some(authority.len())
        })
    }

    /// The route `url` maps to: the first prefix that matches is stripped, then the patterns are
    /// tried most specific first. The fragment is not part of the match.
    ///
    /// `ERR_DEEP_LINKS_NO_ROUTE` when no prefix or pattern matches, or a path segment's
    /// percent-escape does not decode; `ERR_DEEP_LINKS_INVALID_PARAMETER` when the matching
    /// pattern's `parse` refuses a param, which never falls through to a looser pattern.
    pub fn match_url(&self, url: &str) -> Result<RouteMatch, DeepLinksError> {
        let without_fragment = url.split_once('#').map_or(url, |(before, _)| before);
        let (head, query) = match without_fragment.split_once('?') {
            Some((head, query)) => (head, Some(query)),
            None => (without_fragment, None),
        };
        let path = self
            .prefixes
            .iter()
            .find_map(|prefix| strip_prefix(prefix, head).map(|used| &head[used..]))
            .ok_or_else(|| no_route("no prefix of the route table matches the link"))?;
        self.match_parts(path, query)
    }

    /// The route a path (`/orders/42?tab=lines`) maps to, without a prefix.
    pub fn match_path(&self, path: &str) -> Result<RouteMatch, DeepLinksError> {
        let without_fragment = path.split_once('#').map_or(path, |(before, _)| before);
        match without_fragment.split_once('?') {
            Some((path, query)) => self.match_parts(path, Some(query)),
            None => self.match_parts(without_fragment, None),
        }
    }

    fn match_parts(&self, path: &str, query: Option<&str>) -> Result<RouteMatch, DeepLinksError> {
        let mut segments = Vec::new();
        for raw in path.split('/').filter(|segment| !segment.is_empty()) {
            segments.push(decode(raw).ok_or_else(|| {
                no_route("a path segment has a percent-escape that does not decode")
            })?);
        }
        for route in &self.routes {
            let mut found = BTreeMap::new();
            if !walk(&route.parts, &segments, &mut found) {
                continue;
            }
            let mut params = BTreeMap::new();
            for (name, text) in found {
                params.insert(name.clone(), typed(route, &name, &text)?);
            }
            for (name, text) in parse_query(query.unwrap_or_default()) {
                if route_has_path_param(route, &name) {
                    continue;
                }
                let value = typed(route, &name, &text)?;
                params.insert(name, value);
            }
            return Ok(RouteMatch {
                screen: route.screen.clone(),
                params,
            });
        }
        Err(no_route("no pattern of the route table matches the path"))
    }
}

fn route_has_path_param(route: &Route, name: &str) -> bool {
    route
        .parts
        .iter()
        .any(|part| matches!(part, Part::Param { name: n, .. } if n == name))
}

/// A param's value under its route's `parse`; the param's text as a string without one.
fn typed(route: &Route, name: &str, text: &str) -> Result<JsonValue, DeepLinksError> {
    match route.parse.get(name) {
        None => Ok(json!(text)),
        Some(kind) => kind.parse(text).ok_or_else(|| {
            DeepLinksError::new(ErrorKind::InvalidParameter).with_detail(format!(
                "the param `{name}` of `{}` is not a {}",
                route.screen,
                kind.as_str()
            ))
        }),
    }
}

/// Flattens nested `screens` into routes, each child's pattern under its parent's unless `exact`.
fn collect_screens(
    screens: &Map<String, JsonValue>,
    parent: &str,
    routes: &mut Vec<Route>,
) -> Result<(), DeepLinksError> {
    // Sorted, so equally specific patterns tie-break the same whichever map serde_json uses.
    let mut entries: Vec<_> = screens.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));
    for (screen, spec) in entries {
        if screen.is_empty() {
            return Err(invalid("a screen's name is not empty"));
        }
        let mut patterns = Vec::new();
        let mut parse = BTreeMap::new();
        let own_path;
        let mut exact = false;
        let mut children = None;
        match spec {
            JsonValue::String(pattern) => own_path = Some(pattern.clone()),
            JsonValue::Object(fields) => {
                for key in fields.keys() {
                    if !matches!(
                        key.as_str(),
                        "path" | "parse" | "alias" | "exact" | "screens"
                    ) {
                        return Err(invalid(format!(
                            "`{key}` is not a key of a screen's config (path, parse, alias, exact, screens): screens.{screen}"
                        )));
                    }
                }
                own_path = match fields.get("path") {
                    None => None,
                    Some(JsonValue::String(path)) => Some(path.clone()),
                    Some(_) => {
                        return Err(invalid(format!("screens.{screen}: `path` is a string")));
                    }
                };
                exact = match fields.get("exact") {
                    None => false,
                    Some(JsonValue::Bool(exact)) => *exact,
                    Some(_) => {
                        return Err(invalid(format!("screens.{screen}: `exact` is a boolean")));
                    }
                };
                if let Some(value) = fields.get("parse") {
                    let entries = value.as_object().ok_or_else(|| {
                        invalid(format!(
                            "screens.{screen}: `parse` is an object: {{ \"id\": \"integer\" }}"
                        ))
                    })?;
                    for (name, kind) in entries {
                        let kind = kind.as_str().and_then(ParamType::from_name).ok_or_else(|| {
                            invalid(format!(
                                "screens.{screen}: parse.{name} is \"string\", \"number\", \"integer\" or \"boolean\""
                            ))
                        })?;
                        parse.insert(name.clone(), kind);
                    }
                }
                if let Some(value) = fields.get("alias") {
                    let aliases = value.as_array().ok_or_else(|| {
                        invalid(format!("screens.{screen}: `alias` is an array of patterns"))
                    })?;
                    if own_path.is_none() && !aliases.is_empty() {
                        return Err(invalid(format!(
                            "screens.{screen} has an `alias` but no `path`; an alias is another path"
                        )));
                    }
                    for alias in aliases {
                        let alias = alias.as_str().ok_or_else(|| {
                            invalid(format!("screens.{screen}: an alias is a pattern string"))
                        })?;
                        patterns.push(alias.to_owned());
                    }
                }
                if let Some(value) = fields.get("screens") {
                    children = Some(value.as_object().ok_or_else(|| {
                        invalid(format!("screens.{screen}: `screens` is an object"))
                    })?);
                }
            }
            _ => {
                return Err(invalid(format!(
                    "screens.{screen} is a pattern string, or an object: {{ \"path\": \"orders/:id\" }}"
                )));
            }
        }
        let base = if exact { "" } else { parent };
        let full = |pattern: &str| join(base, pattern);
        if let Some(path) = &own_path {
            patterns.insert(0, path.clone());
        }
        for pattern in &patterns {
            let pattern = full(pattern);
            let parts = parse_pattern(&pattern)?;
            let normalized = parts_pattern(&pattern);
            if routes
                .iter()
                .any(|r| r.screen == *screen && r.pattern == normalized)
            {
                continue;
            }
            routes.push(Route {
                screen: screen.clone(),
                pattern: normalized,
                parts,
                parse: parse.clone(),
                order: routes.len(),
            });
        }
        if let Some(children) = children {
            let child_base = match &own_path {
                Some(path) => full(path),
                None => base.to_owned(),
            };
            collect_screens(children, &child_base, routes)?;
        }
    }
    Ok(())
}

fn join(parent: &str, child: &str) -> String {
    match (parent.trim_matches('/'), child.trim_matches('/')) {
        ("", child) => child.to_owned(),
        (parent, "") => parent.to_owned(),
        (parent, child) => format!("{parent}/{child}"),
    }
}

fn parts_pattern(pattern: &str) -> String {
    pattern
        .split('/')
        .filter(|segment| !segment.is_empty())
        .collect::<Vec<_>>()
        .join("/")
}

/// Splits a pattern into parts with `getPatternParts`' rules, refusing what it refuses: a `:`
/// mid-segment, a `(` without a param, an unclosed `(`, a `?` without a param, combined
/// modifiers, a nameless or twice-named param, and a regex that does not compile.
fn parse_pattern(pattern: &str) -> Result<Vec<Part>, DeepLinksError> {
    let bad = |what: &str| invalid(format!("the pattern `{pattern}`: {what}"));
    let chars: Vec<char> = pattern.chars().collect();
    let mut parts = Vec::new();
    let mut names: Vec<String> = Vec::new();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] == '/' {
            i += 1;
            continue;
        }
        if chars[i] != ':' {
            let start = i;
            while i < chars.len() && chars[i] != '/' {
                match chars[i] {
                    ':' => return Err(bad("`:` in the middle of a segment")),
                    '(' => return Err(bad("`(` without a param before it")),
                    ')' => return Err(bad("`)` without a `(` before it")),
                    '?' => return Err(bad("`?` without a param before it")),
                    _ => i += 1,
                }
            }
            let segment: String = chars[start..i].iter().collect();
            if segment == "*" {
                parts.push(Part::Wildcard);
            } else {
                let decoded =
                    decode(&segment).ok_or_else(|| bad("a percent-escape that does not decode"))?;
                parts.push(Part::Static(decoded));
            }
            continue;
        }
        i += 1;
        let start = i;
        while i < chars.len() && !matches!(chars[i], '/' | '(' | ')' | '?' | '+' | '*' | ':') {
            i += 1;
        }
        let name: String = chars[start..i].iter().collect();
        if name.is_empty() {
            return Err(bad("a param has a name"));
        }
        if names.contains(&name) {
            return Err(bad(&format!("the param `:{name}` is named twice")));
        }
        let mut regex = None;
        if i < chars.len() && chars[i] == '(' {
            let (source, end) = regex_source(&chars, i).ok_or_else(|| bad("an unclosed `(`"))?;
            let compiled = Regex::new(&format!("^(?:{source})$"))
                .map_err(|_| bad(&format!("`{source}` is not a regex")))?;
            regex = Some(compiled);
            i = end;
        }
        let mut optional = false;
        let mut repeat = None;
        let mut modifiers = 0;
        while i < chars.len() && chars[i] != '/' {
            match chars[i] {
                '?' => optional = true,
                '+' => repeat = Some(Repeat::OneOrMore),
                '*' => repeat = Some(Repeat::ZeroOrMore),
                ':' => return Err(bad("`:` in the middle of a segment")),
                _ => return Err(bad("text after a param's modifier")),
            }
            modifiers += 1;
            i += 1;
        }
        if modifiers > 1 {
            return Err(bad("param modifiers do not combine"));
        }
        names.push(name.clone());
        parts.push(Part::Param {
            name,
            regex,
            optional,
            repeat,
        });
    }
    Ok(parts)
}

/// The regex between the `(` at `open` and its matching `)`, escapes and classes respected, and
/// the index after the `)`; `None` when it never closes.
fn regex_source(chars: &[char], open: usize) -> Option<(String, usize)> {
    let mut depth = 0usize;
    let mut escaped = false;
    let mut class = false;
    let mut source = String::new();
    for (at, &c) in chars.iter().enumerate().skip(open + 1) {
        if escaped {
            escaped = false;
        } else if c == '\\' {
            escaped = true;
        } else if class {
            class = c != ']';
        } else if c == '[' {
            class = true;
        } else if c == '(' {
            depth += 1;
        } else if c == ')' {
            if depth == 0 {
                return Some((source, at + 1));
            }
            depth -= 1;
        }
        source.push(c);
    }
    None
}

/// Whether `parts` match all of `segments`, collecting each param's decoded text. Repeats and
/// the wildcard take the most segments that still let the rest match.
fn walk(parts: &[Part], segments: &[String], params: &mut BTreeMap<String, String>) -> bool {
    let Some((part, rest)) = parts.split_first() else {
        return segments.is_empty();
    };
    match part {
        Part::Static(text) => segments.first() == Some(text) && walk(rest, &segments[1..], params),
        Part::Wildcard => (0..=segments.len())
            .rev()
            .any(|taken| walk(rest, &segments[taken..], params)),
        Part::Param {
            name,
            regex,
            optional,
            repeat,
        } => {
            let fits = |segment: &String| regex.as_ref().is_none_or(|r| r.is_match(segment));
            let (least, most) = match repeat {
                None => (usize::from(!*optional), 1),
                Some(Repeat::OneOrMore) => (1, segments.len()),
                Some(Repeat::ZeroOrMore) => (0, segments.len()),
            };
            for taken in (least..=most.min(segments.len())).rev() {
                if !segments[..taken].iter().all(fits) {
                    continue;
                }
                let absent = taken == 0 && repeat.is_none();
                if !absent {
                    params.insert(name.clone(), segments[..taken].join("/"));
                }
                if walk(rest, &segments[taken..], params) {
                    return true;
                }
                params.remove(name);
            }
            false
        }
    }
}

/// The query's pairs, keys and values percent-decoded (raw where they do not decode); a later
/// key replaces an earlier one.
fn parse_query(query: &str) -> Vec<(String, String)> {
    let mut pairs: Vec<(String, String)> = Vec::new();
    for pair in query.split('&').filter(|pair| !pair.is_empty()) {
        let (key, value) = pair.split_once('=').unwrap_or((pair, ""));
        let key = decode(key).unwrap_or_else(|| key.to_owned());
        let value = decode(value).unwrap_or_else(|| value.to_owned());
        pairs.retain(|(k, _)| *k != key);
        pairs.push((key, value));
    }
    pairs
}

/// Percent-decodes `text`; `None` for a `%` without two hex digits, or bytes that are not UTF-8.
/// Hex digits decode the same in either case.
fn decode(text: &str) -> Option<String> {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' {
            let hex = text.get(i + 1..i + 3)?;
            if !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
                return None;
            }
            out.push(u8::from_str_radix(hex, 16).ok()?);
            i += 3;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    String::from_utf8(out).ok()
}

fn is_web(scheme: &str) -> bool {
    scheme == "http" || scheme == "https"
}

/// Splits a prefix the way matching walks it. A custom scheme is checked by NSH core's
/// `check_scheme`: a reserved one is `DisallowedScheme`, a malformed one `InvalidArgument`.
fn split_prefix(prefix: &str) -> Result<Prefix, DeepLinksError> {
    if prefix == "*" {
        return Ok(Prefix {
            scheme: String::new(),
            any: true,
            labels: Vec::new(),
            path: String::new(),
            raw: prefix.to_owned(),
        });
    }
    let Some(colon) = prefix.find(':') else {
        return Err(invalid(format!(
            "`{prefix}` is not a prefix: \"scheme://host\", \"scheme://\" or \"*\""
        )));
    };
    let scheme = prefix[..colon].to_ascii_lowercase();
    if let Some(Err(reason)) =
        (!is_web(&scheme)).then(|| nsh_core::url_scheme::check_scheme(&scheme))
    {
        // A well-formed name fails `check_scheme` only for being reserved, and appending a
        // letter keeps a name well-formed: that tells the two refusals apart.
        let reserved =
            !scheme.is_empty() && nsh_core::url_scheme::check_scheme(&format!("{scheme}x")).is_ok();
        let kind = if reserved {
            ErrorKind::DisallowedScheme
        } else {
            ErrorKind::InvalidArgument
        };
        return Err(DeepLinksError::new(kind).with_detail(reason));
    }
    let rest = &prefix[colon + 1..];
    if rest.contains(['?', '#']) || rest.contains(char::is_whitespace) {
        return Err(invalid(format!(
            "`{prefix}`: a prefix is a scheme, a host and a path, with no query or fragment"
        )));
    }
    let rest = rest.trim_start_matches('/');
    let (host, path) = match rest.find('/') {
        Some(at) => (&rest[..at], parts_pattern(&rest[at..])),
        None => (rest, String::new()),
    };
    if is_web(&scheme) && host.is_empty() {
        return Err(invalid(format!(
            "`{prefix}`: an http(s) prefix names its host"
        )));
    }
    let mut labels = Vec::new();
    if !host.is_empty() {
        for label in host.split('.') {
            match label {
                "*" => labels.push(Label::Any),
                "" => return Err(invalid(format!("`{prefix}`: a host label is not empty"))),
                label => labels.push(Label::Lit(label.to_owned())),
            }
        }
    }
    Ok(Prefix {
        scheme,
        any: false,
        labels,
        path: if path.is_empty() {
            path
        } else {
            format!("/{path}")
        },
        raw: prefix.to_owned(),
    })
}

/// How many bytes of `head` (a URL without query or fragment) the prefix takes; `None` when it
/// does not match. Scheme and host compare case-insensitively, the path as written.
fn strip_prefix(prefix: &Prefix, head: &str) -> Option<usize> {
    let colon = head.find(':')?;
    let scheme = &head[..colon];
    if scheme.is_empty() || scheme.contains('/') {
        return None;
    }
    let mut at = colon + 1;
    while head[at..].starts_with('/') {
        at += 1;
    }
    if prefix.any {
        if is_web(&scheme.to_ascii_lowercase()) {
            let rest = &head[at..];
            let host = rest.find('/').unwrap_or(rest.len());
            if host == 0 {
                return None;
            }
            at += host;
        }
        return Some(at);
    }
    if !scheme.eq_ignore_ascii_case(&prefix.scheme) {
        return None;
    }
    if !prefix.labels.is_empty() {
        let rest = &head[at..];
        let end = rest.find('/').unwrap_or(rest.len());
        if match_labels(&prefix.labels, &rest[..end]) != Some(end) {
            return None;
        }
        at += end;
    }
    if !prefix.path.is_empty() {
        let rest = &head[at..];
        if !rest.starts_with(prefix.path.as_str()) {
            return None;
        }
        let after = &rest[prefix.path.len()..];
        if !(after.is_empty() || after.starts_with('/')) {
            return None;
        }
        at += prefix.path.len();
    }
    Some(at)
}

/// Matches `labels` against all of `host`; the bytes used.
fn match_labels(labels: &[Label], host: &str) -> Option<usize> {
    let (first, rest) = labels.split_first()?;
    match first {
        Label::Lit(lit) => {
            let head = host.get(..lit.len())?;
            if !head.eq_ignore_ascii_case(lit) {
                return None;
            }
            if rest.is_empty() {
                return Some(lit.len());
            }
            let after = host.get(lit.len()..)?.strip_prefix('.')?;
            Some(lit.len() + 1 + match_labels(rest, after)?)
        }
        Label::Any => {
            if rest.is_empty() {
                return (!host.is_empty()).then_some(host.len());
            }
            // The longest run of whole labels that lets the rest match.
            for (at, _) in host
                .match_indices('.')
                .collect::<Vec<_>>()
                .into_iter()
                .rev()
            {
                if at == 0 {
                    continue;
                }
                if let Some(more) = match_labels(rest, &host[at + 1..]) {
                    return Some(at + 1 + more);
                }
            }
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(config: &JsonValue) -> RouteTable {
        match RouteTable::from_config(config) {
            Ok(table) => table,
            Err(error) => panic!("{config} is a valid config: {error}"),
        }
    }

    fn screen(table: &RouteTable, url: &str) -> Option<String> {
        table.match_url(url).ok().map(|m| m.screen)
    }

    fn code<T>(answer: Result<T, DeepLinksError>) -> Option<&'static str> {
        answer.err().map(|error| error.code())
    }

    #[test]
    fn a_prefix_of_only_a_scheme_takes_any_host() {
        let routes = table(&json!({ "prefixes": ["scheme://"], "screens": { "S": "some/path" } }));
        for url in [
            "scheme://some/path",
            "scheme:some/path",
            "scheme:///some/path",
        ] {
            assert_eq!(screen(&routes, url).as_deref(), Some("S"), "{url}");
        }
    }

    #[test]
    fn a_host_prefix_takes_only_its_host() {
        let routes = table(&json!({
            "prefixes": ["scheme://example.com"],
            "screens": { "S": "some/path", "Home": "" }
        }));
        assert_eq!(
            screen(&routes, "scheme://example.com/some/path").as_deref(),
            Some("S")
        );
        assert_eq!(
            screen(&routes, "scheme://example.com").as_deref(),
            Some("Home")
        );
        assert_eq!(screen(&routes, "scheme://foo.com/some/path"), None);
        assert_eq!(
            screen(&routes, "scheme://example.com.evil.com/some/path"),
            None
        );
    }

    #[test]
    fn a_wildcard_label_covers_whole_labels() {
        let routes = table(&json!({
            "prefixes": ["https://*.example.com"],
            "screens": { "S": "some/path" }
        }));
        assert!(routes.allows_host("https", "a.example.com"));
        assert!(routes.allows_host("https", "a.b.example.com"));
        assert!(!routes.allows_host("https", "example.com"));
        assert!(!routes.allows_host("https", "aexample.com"));
        assert!(!routes.allows_host("https", "a.example.com.evil.com"));
        assert!(!routes.allows_host("http", "a.example.com"));
    }

    #[test]
    fn a_prefix_path_is_matched_to_a_segment_end() {
        let routes = table(&json!({
            "prefixes": ["https://example.com/shop"],
            "screens": { "S": "orders" }
        }));
        assert_eq!(
            screen(&routes, "https://example.com/shop/orders").as_deref(),
            Some("S")
        );
        assert_eq!(
            screen(&routes, "https://example.com/shop//orders").as_deref(),
            Some("S")
        );
        assert_eq!(screen(&routes, "https://example.com/shopping/orders"), None);
    }

    #[test]
    fn a_host_with_a_port_is_one_authority() {
        let routes = table(&json!({ "prefixes": ["http://127.0.0.1:19000"], "screens": {} }));
        assert!(routes.allows_host("http", "127.0.0.1:19000"));
        assert!(!routes.allows_host("http", "127.0.0.1"));
    }

    #[test]
    fn regexes_with_slashes_and_classes_parse() {
        let routes = table(&json!({
            "prefixes": ["myapp://"],
            "screens": { "S": "x/:id([0-9)]+)/:slug(a|b)" }
        }));
        assert_eq!(screen(&routes, "myapp://x/1)2/a").as_deref(), Some("S"));
        assert_eq!(screen(&routes, "myapp://x/1)2/c"), None);
    }

    #[test]
    fn repeats_take_segments_and_join_them() {
        let routes = table(&json!({
            "prefixes": ["myapp://"],
            "screens": { "Files": "files/:path+", "Docs": "docs/:rest*" }
        }));
        let files = routes.match_url("myapp://files/a/b/c");
        assert_eq!(
            files.ok().and_then(|m| m.params.get("path").cloned()),
            Some(json!("a/b/c"))
        );
        assert_eq!(screen(&routes, "myapp://files"), None);
        let docs = routes.match_url("myapp://docs");
        assert_eq!(
            docs.ok().and_then(|m| m.params.get("rest").cloned()),
            Some(json!(""))
        );
    }

    #[test]
    fn nested_screens_compose_their_parents_path_unless_exact() {
        let routes = table(&json!({
            "prefixes": ["myapp://"],
            "path": "app",
            "screens": {
                "Settings": {
                    "path": "settings",
                    "screens": {
                        "Profile": "profile/:id",
                        "Help": { "path": "help", "exact": true },
                        "Old": { "path": "account", "alias": ["acct"] }
                    }
                }
            }
        }));
        assert_eq!(
            screen(&routes, "myapp://app/settings").as_deref(),
            Some("Settings")
        );
        assert_eq!(
            screen(&routes, "myapp://app/settings/profile/7").as_deref(),
            Some("Profile")
        );
        assert_eq!(screen(&routes, "myapp://help").as_deref(), Some("Help"));
        assert_eq!(
            screen(&routes, "myapp://app/settings/acct").as_deref(),
            Some("Old")
        );
    }

    #[test]
    fn types_parse_strictly() {
        assert_eq!(ParamType::Number.parse("4.5"), Some(json!(4.5)));
        assert_eq!(ParamType::Number.parse("42"), Some(json!(42)));
        assert_eq!(ParamType::Number.parse("1e3"), Some(json!(1000)));
        assert_eq!(ParamType::Integer.parse("-7"), Some(json!(-7)));
        assert_eq!(ParamType::Boolean.parse("true"), Some(json!(true)));
        for text in ["", "+1", "0x10", "Infinity", "NaN", " 1", "1.", ".5", "1e"] {
            assert_eq!(ParamType::Number.parse(text), None, "{text}");
        }
        assert_eq!(ParamType::Integer.parse("4.0"), None);
        assert_eq!(ParamType::Boolean.parse("TRUE"), None);
    }

    #[test]
    fn a_config_the_dialect_does_not_take_is_refused() {
        for config in [
            json!("not an object"),
            json!({ "initialRouteName": "x", "screens": {} }),
            json!({ "prefixes": "https://example.com" }),
            json!({ "prefixes": [1] }),
            json!({ "prefixes": [""] }),
            json!({ "prefixes": ["https"] }),
            json!({ "prefixes": ["https://"] }),
            json!({ "prefixes": ["https://example.com?q"] }),
            json!({ "prefixes": ["https://a..b"] }),
            json!({ "prefixes": ["1app://"] }),
            json!({ "path": "app/:id" }),
            json!({ "path": 1 }),
            json!({ "screens": { "O": { "path": 1 } } }),
            json!({ "screens": { "O": { "path": "o", "other": 1 } } }),
            json!({ "screens": { "O": { "path": "o", "parse": { "id": "int" } } } }),
            json!({ "screens": { "O": { "path": "o", "parse": "x" } } }),
            json!({ "screens": { "O": { "alias": ["o"] } } }),
            json!({ "screens": { "O": { "path": "o", "exact": "yes" } } }),
            json!({ "screens": { "O": 1 } }),
            json!({ "screens": { "": "x" } }),
        ] {
            assert_eq!(
                code(RouteTable::from_config(&config)),
                Some("ERR_DEEP_LINKS_INVALID_ARGUMENT"),
                "{config}"
            );
        }
    }

    #[test]
    fn a_pattern_the_grammar_does_not_take_is_refused() {
        for pattern in [
            "chat/a:b",
            "chat/:a(b",
            "chat/a(b)",
            "chat/:a)",
            "chat/a?",
            "chat/:?",
            "chat/:a?+",
            "chat/:a+?",
            "chat/:",
            "chat/:a/:a",
            "chat/:a([)",
            "chat/:a+x",
        ] {
            let config = json!({ "screens": { "S": pattern } });
            assert_eq!(
                code(RouteTable::from_config(&config)),
                Some("ERR_DEEP_LINKS_INVALID_ARGUMENT"),
                "{pattern}"
            );
        }
    }
}
