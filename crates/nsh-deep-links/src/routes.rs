//! The route table: prefixes and patterns map a link's path to a screen, ported from React
//! Navigation's linking config. Custom regexes, repeat modifiers and nested screens are not
//! ported: a param is typed in `params` instead, and what the dialect does not take is refused
//! with `ERR_DEEP_LINKS_INVALID_PARAMETER`. Unlike React Navigation, scheme and host match
//! case-insensitively, as URLs do; paths stay case-sensitive.

use std::collections::BTreeMap;

use nsh_core::JsonValue;
use percent_encoding::percent_decode_str;
use serde_json::json;

use crate::error::{DeepLinksError, ErrorKind};

/// How a param's value is validated and the JSON it carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParamType {
    /// Any text; the param arrives as a JSON string.
    Str,
    /// A decimal number, finite; the param arrives as a JSON number.
    Number,
}

impl ParamType {
    /// The name the config writes: `"string"` or `"number"`.
    pub fn as_str(self) -> &'static str {
        match self {
            ParamType::Str => "string",
            ParamType::Number => "number",
        }
    }

    fn from_config(name: &str) -> Option<Self> {
        match name {
            "string" => Some(ParamType::Str),
            "number" => Some(ParamType::Number),
            _ => None,
        }
    }
}

/// One segment of a pattern.
#[derive(Debug, Clone, PartialEq, Eq)]
enum PatternPart {
    /// Literal text, percent-decoded, matched case-sensitively.
    Static(String),
    /// A named param, optional (`?`) or not; typed by the route's `params`.
    Param { name: String, optional: bool },
    /// `*`: matches every remaining segment. Always the last part.
    Wildcard,
}

impl PatternPart {
    fn rank(&self) -> u8 {
        // React Navigation's order: static, then params, then the end, then wildcards.
        match self {
            PatternPart::Static(_) => 0,
            PatternPart::Param { .. } => 2,
            PatternPart::Wildcard => 8,
        }
    }
}

/// One screen's entry: its pattern, split into parts.
#[derive(Debug, Clone)]
struct Route {
    screen: String,
    pattern: String,
    parts: Vec<PatternPart>,
    /// The params config: types for path params, and for query params of the same names.
    kinds: BTreeMap<String, ParamType>,
    /// The insertion order, kept as the tie-break of the specificity sort.
    order: usize,
}

/// A host label of a prefix: literal text, or `*` for any run of labels.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Label {
    Any,
    Lit(String),
}

/// A link prefix, split the way matching walks it.
#[derive(Debug, Clone)]
struct Prefix {
    /// Lower-case, no colon. Empty for the `*` prefix.
    scheme: String,
    /// `scheme:`, what the URL must start with.
    head: String,
    any_scheme: bool,
    /// Host labels; empty when the prefix names no host (`myapp://`).
    labels: Vec<Label>,
    /// After the host: empty, or `/` and what follows.
    path: String,
    /// The prefix as configured.
    raw: String,
}

/// A matched route: the screen, and its params.
#[derive(Debug, Clone, PartialEq)]
pub struct RouteMatch {
    /// The config key the pattern belongs to.
    pub screen: String,
    /// Path params and query params, the path's first.
    pub params: BTreeMap<String, JsonValue>,
}

/// The validated linking config of an app: prefixes, and the patterns that map paths to screens.
#[derive(Debug, Clone, Default)]
pub struct RouteTable {
    prefixes: Vec<Prefix>,
    routes: Vec<Route>,
}

impl RouteTable {
    /// A table with no prefixes and no routes; every link is without a route.
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds the table from a config:
    ///
    /// ```json
    /// {
    ///   "prefixes": ["https://example.com", "myapp://"],
    ///   "screens": {
    ///     "Orders": "orders/:id",
    ///     "Chat": { "path": "chat/:author/:id", "params": { "id": "number" } }
    ///   }
    /// }
    /// ```
    ///
    /// Every shape the dialect does not take is refused with `ERR_DEEP_LINKS_INVALID_PARAMETER`.
    pub fn from_config(value: &JsonValue) -> Result<Self, DeepLinksError> {
        let invalid = |detail: String| {
            DeepLinksError::new(ErrorKind::InvalidParameter).with_detail(detail)
        };
        let object = value.as_object().ok_or_else(|| {
            invalid("the config is an object: { \"prefixes\": [...], \"screens\": { ... } }".into())
        })?;
        for key in object.keys() {
            if key != "prefixes" && key != "screens" {
                return Err(invalid(format!(
                    "`{key}` is not a key of a route config (prefixes, screens)"
                )));
            }
        }
        let prefixes = match object.get("prefixes") {
            None => Vec::new(),
            Some(value) => {
                let list = value.as_array().ok_or_else(|| {
                    invalid("\"prefixes\" is an array: [\"https://example.com\", \"myapp://\"]".into())
                })?;
                let mut parsed = Vec::with_capacity(list.len());
                for prefix in list {
                    let prefix = prefix.as_str().ok_or_else(|| {
                        invalid("a prefix is a string: \"https://example.com\", or \"*\"".into())
                    })?;
                    parsed.push(split_prefix(prefix)?);
                }
                parsed
            }
        };

        let mut routes = Vec::new();
        if let Some(screens) = object
            .get("screens")
            .map(|value| {
                value
                    .as_object()
                    .ok_or_else(|| invalid("\"screens\" is an object: { \"Orders\": \"orders/:id\" }".into()))
            })
            .transpose()?
        {
            if screens.keys().any(|screen| screen.is_empty()) {
                return Err(invalid("a screen's name is not empty".into()));
            }
            for (screen, spec) in screens {
                let (pattern, kinds) = screen_spec(screen, spec)?;
                let parts = parse_pattern(&pattern)?;
                routes.push(Route {
                    screen: screen.clone(),
                    pattern,
                    parts,
                    kinds,
                    order: routes.len(),
                });
            }
        }
        for (i, route) in routes.iter().enumerate() {
            if routes[..i].iter().any(|other| other.pattern == route.pattern) {
                return Err(invalid(format!(
                    "the pattern `{}` is listed for more than one screen; patterns are unique",
                    route.pattern
                )));
            }
        }
        // Most specific first: a static segment beats a param at the same position.
        routes.sort_by(|a, b| {
            let ranks = a.parts.iter().map(PatternPart::rank).collect::<Vec<_>>();
            let other = b.parts.iter().map(PatternPart::rank).collect::<Vec<_>>();
            for i in 0..ranks.len().max(other.len()) {
                let left = ranks.get(i).copied().unwrap_or(5);
                let right = other.get(i).copied().unwrap_or(5);
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

    /// The custom schemes the prefixes claim, lower-case, each once. Verified web links are
    /// `http(s)`; every other prefix's scheme is one the app claims, checked by NSH core.
    pub fn claimed_schemes(&self) -> Vec<String> {
        let mut schemes: Vec<String> = Vec::new();
        for prefix in &self.prefixes {
            if prefix.any_scheme || prefix.scheme == "http" || prefix.scheme == "https" {
                continue;
            }
            if !schemes.contains(&prefix.scheme) {
                schemes.push(prefix.scheme.clone());
            }
        }
        schemes
    }

    /// Whether an `http(s)` link's host is one the prefixes allow, subdomains included where a
    /// prefix has a `*` label. Ports are not part of `host`.
    pub fn allows_host(&self, host: &str) -> bool {
        self.prefixes.iter().any(|prefix| {
            (prefix.scheme == "http" || prefix.scheme == "https")
                && match_labels(&prefix.labels, host).is_some_and(|used| used == host.len())
        })
    }

    /// The path of a link under the table's prefixes, query kept: the first matching prefix
    /// wins, as React Navigation's `extractPathFromURL`. `None` when no prefix matches.
    pub fn extract_path(&self, url: &str) -> Option<String> {
        let (origin_and_path, query) = match url.split_once('?') {
            Some((before, after)) => (before, Some(after)),
            None => (url, None),
        };
        for prefix in &self.prefixes {
            let Some(used) = strip_prefix(prefix, origin_and_path) else {
                continue;
            };
            let mut path = collapse_slashes(&origin_and_path[used..]);
            if let Some(query) = query {
                path.push('?');
                path.push_str(query);
            }
            if !path.starts_with('/') {
                path.insert(0, '/');
            }
            return Some(path);
        }
        None
    }

    /// The route a path maps to, most specific pattern first. Query params join the path
    /// params; a param typed `number` that is not one makes the pattern not match.
    pub fn match_path(&self, path: &str) -> Option<RouteMatch> {
        let (path, query) = match path.split_once('?') {
            Some((before, after)) => (before, Some(after)),
            None => (path, None),
        };
        let segments: Vec<String> = path
            .split('/')
            .filter(|segment| !segment.is_empty())
            .map(decode)
            .collect();
        for route in &self.routes {
            let mut params = BTreeMap::new();
            if !walk(&route.parts, 0, &segments, 0, route, &mut params) {
                continue;
            }
            if let Some(query) = query {
                let mut failed = false;
                for (key, value) in parse_query(query) {
                    if params.contains_key(&key) {
                        continue;
                    }
                    match route.kinds.get(&key).copied() {
                        // A typed query param that is not one fails the pattern, as React
                        // Navigation's parse functions do.
                        Some(ParamType::Number) => match number_value(&value) {
                            Some(number) => {
                                params.insert(key, number);
                            }
                            None => failed = true,
                        },
                        Some(ParamType::Str) | None => {
                            params.insert(key, json!(value));
                        }
                    }
                    if failed {
                        break;
                    }
                }
                if failed {
                    continue;
                }
            }
            return Some(RouteMatch {
                screen: route.screen.clone(),
                params,
            });
        }
        None
    }

    /// The route a full URL maps to: its prefix's path against the patterns. `NoRoute` when no
    /// prefix matches or no pattern does.
    pub fn match_url(&self, url: &str) -> Result<RouteMatch, DeepLinksError> {
        match self.extract_path(url) {
            None => Err(DeepLinksError::new(ErrorKind::NoRoute)
                .with_detail("no prefix matches the URL")),
            Some(path) => self.match_path(&path).ok_or_else(|| {
                DeepLinksError::new(ErrorKind::NoRoute).with_detail("no pattern matches the path")
            }),
        }
    }
}

/// The `path` and `params` of one screens entry.
fn screen_spec(
    screen: &str,
    spec: &JsonValue,
) -> Result<(String, BTreeMap<String, ParamType>), DeepLinksError> {
    let invalid = |detail: String| {
        DeepLinksError::new(ErrorKind::InvalidParameter).with_detail(detail)
    };
    let (pattern, params) = match spec {
        JsonValue::String(pattern) => (pattern.clone(), None),
        JsonValue::Object(fields) => {
            for key in fields.keys() {
                if key != "path" && key != "params" {
                    return Err(invalid(format!(
                        "`{key}` is not a key of a screen's config (path, params): screens.{screen}"
                    )));
                }
            }
            let pattern = match fields.get("path") {
                Some(JsonValue::String(pattern)) => pattern.clone(),
                Some(_) => {
                    return Err(invalid(format!("screens.{screen}: `path` is a string pattern")))
                }
                None => {
                    return Err(invalid(format!(
                        "screens.{screen} needs a `path`, or the pattern alone as a string"
                    )))
                }
            };
            (pattern, fields.get("params"))
        }
        _ => {
            return Err(invalid(format!(
                "screens.{screen} is a pattern string, or an object: {{ \"path\": \"orders/:id\" }}"
            )))
        }
    };
    let mut kinds = BTreeMap::new();
    if let Some(params) = params {
        let fields = params.as_object().ok_or_else(|| {
            invalid(format!(
                "screens.{screen}: `params` is an object: {{ \"id\": \"number\" }}"
            ))
        })?;
        for (name, kind) in fields {
            let kind = kind
                .as_str()
                .and_then(ParamType::from_config)
                .ok_or_else(|| {
                    invalid(format!("screens.{screen}: params.{name} is \"string\" or \"number\""))
                })?;
            kinds.insert(name.clone(), kind);
        }
    }
    Ok((pattern, kinds))
}

/// Splits a pattern into parts, refusing everything the dialect does not take.
fn parse_pattern(pattern: &str) -> Result<Vec<PatternPart>, DeepLinksError> {
    let invalid = |detail: String| {
        DeepLinksError::new(ErrorKind::InvalidParameter).with_detail(detail)
    };
    let mut parts = Vec::new();
    let mut names: Vec<&str> = Vec::new();
    let mut wildcard = None;
    for segment in pattern.split('/') {
        if segment.is_empty() {
            continue;
        }
        if segment == "*" {
            if !parts.is_empty() {
                return Err(invalid(format!(
                    "`{pattern}`: `*` matches every remaining segment, so it is the last one"
                )));
            }
            wildcard = Some(parts.len());
            parts.push(PatternPart::Wildcard);
            continue;
        }
        if let Some(name) = segment.strip_prefix(':') {
            if name.contains('(') || name.contains(')') {
                return Err(invalid(format!(
                    "`{segment}`: custom regexes are not ported; type the param in `params`"
                )));
            }
            let (name, optional) = match name.strip_suffix('?') {
                Some(name) => (name, true),
                None => (name, false),
            };
            if name.ends_with('+') || name.ends_with('*') {
                return Err(invalid(format!(
                    "`{segment}`: repeat modifiers are not ported; params take one segment"
                )));
            }
            if name.is_empty() {
                return Err(invalid(format!("`{segment}`: a param has a name")));
            }
            if names.contains(&name) {
                return Err(invalid(format!(
                    "`{pattern}`: the param `:{name}` is named twice"
                )));
            }
            names.push(name);
            parts.push(PatternPart::Param {
                name: name.to_owned(),
                optional,
            });
        } else {
            if segment.contains(':') || segment.contains('?') {
                return Err(invalid(format!(
                    "`{segment}`: `:` starts a param, and `?` ends an optional one"
                )));
            }
            parts.push(PatternPart::Static(decode(segment)));
        }
    }
    if wildcard.is_some_and(|at| at + 1 != parts.len()) {
        return Err(invalid(format!(
            "`{pattern}`: `*` matches every remaining segment, so it is the last one"
        )));
    }
    Ok(parts)
}

/// Splits a prefix the way matching walks it, checking the scheme through NSH core.
fn split_prefix(prefix: &str) -> Result<Prefix, DeepLinksError> {
    let invalid = |detail: String| {
        DeepLinksError::new(ErrorKind::InvalidParameter).with_detail(detail)
    };
    if prefix == "*" {
        return Ok(Prefix {
            scheme: String::new(),
            head: String::new(),
            any_scheme: true,
            labels: Vec::new(),
            path: String::new(),
            raw: prefix.to_owned(),
        });
    }
    let Some(colon) = prefix.find(':') else {
        return Err(invalid(format!(
            "`{prefix}` is a prefix: \"scheme://host\", or \"*\""
        )));
    };
    let scheme = prefix[..colon].to_ascii_lowercase();
    if scheme != "http" && scheme != "https" {
        // The reserved schemes (http included) an app may not claim are still its verified
        // web-link prefixes; every other scheme is the app's own, which NSH core checks.
        if let Err(reason) = nsh_core::url_scheme::check_scheme(&scheme) {
            return Err(invalid(reason));
        }
    }
    let rest = &prefix[colon + 1..];
    if rest.contains('?') || rest.contains('#') {
        return Err(invalid(format!(
            "`{prefix}`: a prefix is scheme and host, with no query or fragment"
        )));
    }
    let rest = collapse_slashes(rest);
    let rest = rest.strip_prefix('/').unwrap_or(&rest);
    let (host, path) = match rest.find('/') {
        Some(at) => (&rest[..at], rest[at..].to_owned()),
        None => (rest, String::new()),
    };
    let mut labels = Vec::new();
    if !host.is_empty() {
        for label in host.split('.') {
            if label == "*" {
                labels.push(Label::Any);
            } else if label.is_empty() {
                return Err(invalid(format!("`{prefix}`: a host label is not empty")));
            } else {
                labels.push(Label::Lit(label.to_owned()));
            }
        }
    }
    Ok(Prefix {
        head: format!("{scheme}:"),
        scheme,
        any_scheme: false,
        labels,
        path,
        raw: prefix.to_owned(),
    })
}

/// How many bytes of `url` the prefix consumes; `None` when it does not match. Scheme and
/// host compare case-insensitively; the path compares as written.
fn strip_prefix(prefix: &Prefix, url: &str) -> Option<usize> {
    if prefix.any_scheme {
        return strip_any_scheme(url);
    }
    let mut at = starts_with_ci(url, &prefix.head)?;
    while url[at..].starts_with('/') {
        at += 1;
    }
    let used = if prefix.labels.is_empty() {
        0
    } else {
        match_labels(&prefix.labels, &url[at..])?
    };
    at += used;
    if prefix.path.is_empty() {
        // A host that ends here must end the URL's host too: `example.com.evil.com` is not
        // `example.com`.
        if !prefix.labels.is_empty()
            && !matches!(
                url.as_bytes().get(at),
                None | Some(b'/') | Some(b'?') | Some(b'#')
            )
        {
            return None;
        }
        return Some(at);
    }
    if !url.get(at..)?.starts_with(prefix.path.as_str()) {
        return None;
    }
    at += prefix.path.len();
    if !matches!(
        url.as_bytes().get(at),
        None | Some(b'/') | Some(b'?') | Some(b'#')
    ) {
        return None;
    }
    Some(at)
}

/// The `*` prefix: any `http(s)` URL, or any URL of another scheme (`scheme:path`, with any
/// number of slashes), as React Navigation's wildcard.
fn strip_any_scheme(url: &str) -> Option<usize> {
    for scheme in ["http://", "https://"] {
        if let Some(used) = starts_with_ci(url, scheme) {
            let rest = &url[used..];
            let authority = rest.find('/').unwrap_or(rest.len());
            if authority == 0 {
                return None;
            }
            return Some(used + authority);
        }
    }
    // Any other scheme: a `:` before the first `/`, then the optional `//` after it.
    let head = url.split('/').next()?;
    let colon = head.find(':')?;
    if colon == 0 {
        return None;
    }
    let after = &url[colon + 1..];
    Some(colon + 1 + usize::from(after.starts_with("//")))
}

/// Matches `labels` against the head of `s`, `*` covering a run of labels; the bytes used.
fn match_labels(labels: &[Label], s: &str) -> Option<usize> {
    let (first, rest) = labels.split_first()?;
    match first {
        Label::Lit(lit) => {
            let used = starts_with_ci(s, lit)?;
            if rest.is_empty() {
                return Some(used);
            }
            // Labels are separated by one dot, as the prefix writes them.
            let after = s.get(used..)?;
            if !after.starts_with('.') {
                return None;
            }
            let more = match_labels(rest, &after[1..])?;
            Some(used + 1 + more)
        }
        Label::Any => {
            let run = s.find(['/', '?', '#']).unwrap_or(s.len());
            if rest.is_empty() {
                return (run > 0).then_some(run);
            }
            // Greedy, as the regex is: the longest run that still lets the rest match.
            for split in (1..=run).rev() {
                if s.as_bytes()[split - 1] == b'.' {
                    if let Some(more) = match_labels(rest, &s[split..]) {
                        return Some(split + more);
                    }
                }
            }
            None
        }
    }
}

/// Whether a pattern matches the segments, collecting its params. An optional param that is
/// present but of the wrong type fails the pattern; one that is absent is simply not in the
/// params.
fn walk(
    parts: &[PatternPart],
    at: usize,
    segments: &[String],
    used: usize,
    route: &Route,
    params: &mut BTreeMap<String, JsonValue>,
) -> bool {
    let Some(part) = parts.get(at) else {
        return used == segments.len();
    };
    match part {
        PatternPart::Static(text) => match segments.get(used) {
            Some(segment) if segment == text => {
                walk(parts, at + 1, segments, used + 1, route, params)
            }
            _ => false,
        },
        PatternPart::Param { name, optional } => {
            let kind = route.kinds.get(name).copied().unwrap_or(ParamType::Str);
            if let Some(segment) = segments.get(used) {
                let Some(value) = param_value(kind, segment) else {
                    return false;
                };
                params.insert(name.clone(), value);
                if walk(parts, at + 1, segments, used + 1, route, params) {
                    return true;
                }
                params.remove(name);
            }
            *optional && walk(parts, at + 1, segments, used, route, params)
        }
        PatternPart::Wildcard => true,
    }
}

/// A param's value, `None` when the text is not of the type.
fn param_value(kind: ParamType, raw: &str) -> Option<JsonValue> {
    match kind {
        ParamType::Str => Some(json!(raw)),
        ParamType::Number => number_value(raw),
    }
}

/// A finite JSON number, whole when it is one.
fn number_value(raw: &str) -> Option<JsonValue> {
    let number: f64 = raw.parse().ok()?;
    if !number.is_finite() {
        return None;
    }
    if number.fract() == 0.0 && (i64::MIN as f64..=i64::MAX as f64).contains(&number) {
        Some(json!(number as i64))
    } else {
        Some(json!(number))
    }
}

/// The query's pairs, keys and values percent-decoded.
fn parse_query(query: &str) -> Vec<(String, String)> {
    query
        .split('&')
        .filter(|pair| !pair.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((key, value)) => (decode(key), decode(value)),
            None => (decode(pair), String::new()),
        })
        .collect()
}

/// Percent-decodes, replacing malformed text with U+FFFD rather than dropping the link.
fn decode(text: &str) -> String {
    percent_decode_str(text).decode_utf8_lossy().into_owned()
}

/// Collapses runs of `/` into one.
fn collapse_slashes(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut slash = false;
    for char in text.chars() {
        if char == '/' {
            if !slash {
                out.push(char);
            }
            slash = true;
        } else {
            out.push(char);
            slash = false;
        }
    }
    out
}

/// Some(prefix.len()) when `url` starts with `prefix` (ASCII case-insensitive).
fn starts_with_ci(url: &str, prefix: &str) -> Option<usize> {
    url.get(..prefix.len())
        .filter(|head| head.eq_ignore_ascii_case(prefix))
        .map(|_| prefix.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn table(config: &JsonValue) -> RouteTable {
        RouteTable::from_config(config).ok().expect("a valid config")
    }

    fn path(prefixes: &[&str], url: &str) -> Option<String> {
        table(&json!({ "prefixes": prefixes, "screens": {} })).extract_path(url)
    }

    #[test]
    fn a_prefix_of_only_a_scheme_takes_any_host() {
        assert_eq!(
            path(&["scheme://"], "scheme://some/path"),
            Some("/some/path".into())
        );
        assert_eq!(
            path(&["scheme://"], "scheme:some/path"),
            Some("/some/path".into())
        );
        assert_eq!(
            path(&["scheme://"], "scheme:///some/path"),
            Some("/some/path".into())
        );
        assert_eq!(path(&["scheme://"], "scheme://"), Some("/".into()));
        assert_eq!(
            path(&["scheme:"], "scheme://some/path"),
            Some("/some/path".into())
        );
        assert_eq!(
            path(&["scheme:"], "scheme:///some/path"),
            Some("/some/path".into())
        );
    }

    #[test]
    fn a_host_prefix_takes_only_its_host() {
        assert_eq!(
            path(&["scheme://example.com"], "scheme://example.com/some/path"),
            Some("/some/path".into())
        );
        assert_eq!(
            path(&["scheme://example.com"], "scheme:example.com/some/path"),
            Some("/some/path".into())
        );
        assert_eq!(
            path(&["scheme://example.com"], "scheme:///example.com/some/path"),
            Some("/some/path".into())
        );
        assert_eq!(
            path(&["scheme://example.com"], "scheme://example.com"),
            Some("/".into())
        );
        assert_eq!(
            path(&["scheme://example.com"], "scheme://example.com/"),
            Some("/".into())
        );
        assert_eq!(
            path(&["scheme://example.com"], "scheme://foo.com/some/path"),
            None
        );
        assert_eq!(
            path(&["scheme://example.com"], "scheme://example.com.evil.com/x"),
            None
        );
        assert_eq!(path(&["scheme://foo"], "scheme://some/path"), None);
    }

    #[test]
    fn a_wildcard_label_covers_a_run_of_labels() {
        assert_eq!(
            path(&["scheme://*.example.com"], "scheme://test.example.com/some/path"),
            Some("/some/path".into())
        );
        assert_eq!(
            path(&["scheme://*.example.com"], "scheme://a.b.example.com/some/path"),
            Some("/some/path".into())
        );
        assert_eq!(
            path(&["scheme://*.example.com"], "scheme://test.foo.com/some/path"),
            None
        );
        assert_eq!(
            path(&["https://*.example.com"], "https://test.example.com.evil.com/some/path"),
            None
        );
    }

    #[test]
    fn a_prefix_path_is_matched_to_its_end() {
        assert_eq!(
            path(&["scheme://example.com/test"], "scheme://example.com/test/some/path"),
            Some("/some/path".into())
        );
        assert_eq!(
            path(&["scheme://example.com/test"], "scheme://example.com/test//some/path"),
            Some("/some/path".into())
        );
        assert_eq!(
            path(&["scheme://example.com/test"], "scheme://example.com/test/some//path"),
            Some("/some/path".into())
        );
        assert_eq!(
            path(&["https://example.com/app"], "https://example.com/application/path"),
            None
        );
    }

    #[test]
    fn the_query_and_fragment_stay_with_the_path() {
        assert_eq!(
            path(&["scheme://example.com/test"], "scheme://example.com/test?foo=bar"),
            Some("/?foo=bar".into())
        );
        assert_eq!(
            path(&["scheme://example.com/test"], "scheme://example.com/test#section"),
            Some("/#section".into())
        );
        assert_eq!(
            path(&["https://mysite.com"], "https://mysite.com/readPolicy?url=https://test.com?param=1"),
            Some("/readPolicy?url=https://test.com?param=1".into())
        );
    }

    #[test]
    fn an_ip_host_with_a_port_is_one_host() {
        assert_eq!(
            path(&["http://127.0.0.1:19000"], "http://127.0.0.1:19000/some/path"),
            Some("/some/path".into())
        );
        assert_eq!(
            path(&["exp://127.0.0.1:19000"], "exp://127.0.0.1:19000/--/simple-stack"),
            Some("/--/simple-stack".into())
        );
        assert_eq!(
            path(&["http://127.0.0.1:19000"], "http://127.0.0.1:19000"),
            Some("/".into())
        );
    }

    #[test]
    fn the_star_prefix_takes_any_url_with_a_scheme() {
        assert_eq!(path(&["*"], "myapp:some/path"), Some("/some/path".into()));
        assert_eq!(path(&["*"], "myapp://some/path"), Some("/some/path".into()));
        assert_eq!(path(&["*"], "myapp:///some/path"), Some("/some/path".into()));
        assert_eq!(
            path(&["*"], "https://example.com/path?param=value"),
            Some("/path?param=value".into())
        );
        assert_eq!(path(&["*"], "https://example.com"), Some("/".into()));
        assert_eq!(path(&["*"], "http://127.0.0.1:19000"), Some("/".into()));
        assert_eq!(path(&["*"], "some/path/without/scheme"), None);
    }

    #[test]
    fn the_scheme_and_host_match_case_insensitively_and_the_path_keeps_its_case() {
        assert_eq!(
            path(&["https://Example.com"], "HTTPS://example.com/Orders/42"),
            Some("/Orders/42".into())
        );
        assert_eq!(
            path(&["myapp://Example.com"], "MYAPP://EXAMPLE.com/x"),
            Some("/x".into())
        );
        assert_eq!(
            path(&["https://example.com/Orders"], "https://example.com/orders/42"),
            None
        );
    }

    #[test]
    fn a_static_segment_beats_a_param_at_the_same_position() {
        let routes = table(&json!({
            "prefixes": ["https://example.com"],
            "screens": { "New": "orders/new", "Order": "orders/:id" }
        }));
        let new = routes.match_path("/orders/new");
        assert_eq!(new.map(|m| m.screen), Some("New".into()));
        let order = routes.match_path("/orders/42");
        assert_eq!(order.as_ref().map(|m| m.screen.clone()), Some("Order".into()));
        assert_eq!(
            order.and_then(|m| m.params.get("id").cloned()),
            Some(json!("42"))
        );
    }

    #[test]
    fn a_pattern_matches_only_whole_paths() {
        let routes = table(&json!({
            "prefixes": ["myapp://"],
            "screens": { "Order": "orders/:id" }
        }));
        assert!(routes.match_path("/orders/42").is_some());
        assert!(routes.match_path("/orders/42/lines").is_none());
        assert!(routes.match_path("/orders").is_none());
        assert!(routes.match_path("/orders/42/").is_some());
    }

    #[test]
    fn the_root_path_matches_an_empty_pattern() {
        let routes = table(&json!({
            "prefixes": ["https://example.com"],
            "screens": { "Home": "" }
        }));
        assert_eq!(routes.match_path("/").map(|m| m.screen), Some("Home".into()));
        assert!(routes.match_path("/other").is_none());
    }

    #[test]
    fn a_number_param_arrives_as_a_number_or_the_pattern_passes() {
        let routes = table(&json!({
            "prefixes": ["myapp://"],
            "screens": {
                "Order": { "path": "orders/:id", "params": { "id": "number" } },
                "Catch": "*"
            }
        }));
        let order = routes.match_path("/orders/42");
        assert_eq!(order.as_ref().map(|m| m.screen.clone()), Some("Order".into()));
        assert_eq!(
            order.and_then(|m| m.params.get("id").cloned()),
            Some(json!(42))
        );
        // A present param of the wrong type fails the pattern; the next one takes the path.
        assert_eq!(
            routes.match_path("/orders/abc").map(|m| m.screen),
            Some("Catch".into())
        );
    }

    #[test]
    fn query_params_join_the_path_params_and_the_path_wins() {
        let routes = table(&json!({
            "prefixes": ["https://example.com"],
            "screens": { "Order": "orders/:id" }
        }));
        let params = routes
            .match_path("/orders/42?tab=lines&id=99&flag")
            .map(|m| m.params);
        let params = params.as_ref();
        assert_eq!(params.get("id"), Some(&json!("42")));
        assert_eq!(params.get("tab"), Some(&json!("lines")));
        assert_eq!(params.get("flag"), Some(&json!("")));
    }

    #[test]
    fn params_are_percent_decoded() {
        let routes = table(&json!({
            "prefixes": ["myapp://"],
            "screens": { "Chat": "chat/:author" }
        }));
        let author = routes
            .match_path("/chat/jane%20doe")
            .and_then(|m| m.params.get("author").cloned());
        assert_eq!(author, Some(json!("jane doe")));
    }

    #[test]
    fn an_optional_param_matches_a_segment_or_none() {
        let routes = table(&json!({
            "prefixes": ["myapp://"],
            "screens": { "Chat": "chat/:author?/messages" }
        }));
        let with = routes.match_path("/chat/jane/messages");
        assert_eq!(
            with.as_ref().and_then(|m| m.params.get("author")).cloned(),
            Some(json!("jane"))
        );
        let without = routes.match_path("/chat/messages");
        assert_eq!(without.map(|m| m.screen), Some("Chat".into()));
        assert!(
            without
                .and_then(|m| m.params.get("author").cloned())
                .is_none()
        );
    }

    #[test]
    fn a_wildcard_pattern_takes_every_remaining_segment() {
        let routes = table(&json!({
            "prefixes": ["myapp://"],
            "screens": { "All": "*", "Order": "orders/:id" }
        }));
        assert_eq!(
            routes.match_path("/deeply/nested/path").map(|m| m.screen),
            Some("All".into())
        );
        assert_eq!(routes.match_path("/").map(|m| m.screen), Some("All".into()));
    }

    #[test]
    fn a_url_routes_through_its_prefix() {
        let routes = table(&json!({
            "prefixes": ["https://example.com/shop", "myapp://"],
            "screens": { "Order": "orders/:id" }
        }));
        assert_eq!(
            routes
                .match_url("https://example.com/shop/orders/42")
                .map(|m| m.screen),
            Some("Order".into())
        );
        assert_eq!(
            routes.match_url("myapp://orders/42").map(|m| m.screen),
            Some("Order".into())
        );
        let no_prefix = routes.match_url("https://other.com/orders/42");
        assert_eq!(no_prefix.err().code(), "ERR_DEEP_LINKS_NO_ROUTE");
        let no_pattern = routes.match_url("myapp://invoices/42");
        assert_eq!(no_pattern.err().code(), "ERR_DEEP_LINKS_NO_ROUTE");
    }

    #[test]
    fn custom_schemes_come_from_the_prefixes_and_are_checked_by_core() {
        let routes = table(&json!({
            "prefixes": ["https://example.com", "MyApp://", "myapp://deep"],
            "screens": {}
        }));
        assert_eq!(routes.claimed_schemes(), vec!["myapp".to_owned()]);
        assert_eq!(routes.prefixes().len(), 3);
        let reserved = RouteTable::from_config(&json!({
            "prefixes": ["mailto://"], "screens": {}
        }));
        assert_eq!(
            reserved.err().code(),
            "ERR_DEEP_LINKS_INVALID_PARAMETER"
        );
    }

    #[test]
    fn an_http_prefix_allows_its_host_and_its_subdomains_where_starred() {
        let routes = table(&json!({
            "prefixes": ["https://example.com", "https://*.shop.example.com"],
            "screens": {}
        }));
        assert!(routes.allows_host("example.com"));
        assert!(routes.allows_host("EXAMPLE.com"));
        assert!(!routes.allows_host("example.com.evil.com"));
        assert!(routes.allows_host("a.shop.example.com"));
        assert!(!routes.allows_host("shop.example.com"));
        assert!(!routes.allows_host("a.b.shop.example.com"));
    }

    #[test]
    fn a_config_the_dialect_does_not_take_is_refused() {
        for config in [
            json!("not an object"),
            json!({ "path": "/x", "screens": {} }),
            json!({ "prefixes": "https://example.com", "screens": {} }),
            json!({ "prefixes": [1], "screens": {} }),
            json!({ "prefixes": [""], "screens": {} }),
            json!({ "prefixes": ["https"], "screens": {} }),
            json!({ "prefixes": ["https://example.com?q"], "screens": {} }),
            json!({ "prefixes": ["https://a..b"], "screens": {} }),
            json!({ "screens": { "O": { "path": 1 } } }),
            json!({ "screens": { "O": { "path": "o", "other": 1 } } }),
            json!({ "screens": { "O": { "params": { "id": "int" } } } }),
            json!({ "screens": { "O": { "path": "o", "params": "x" } } }),
            json!({ "screens": { "O": 1 } }),
            json!({ "screens": { "": "x" } }),
        ] {
            assert!(RouteTable::from_config(&config).is_err(), "{config}");
        }
    }

    #[test]
    fn a_pattern_the_dialect_does_not_take_is_refused() {
        for pattern in [
            "chat/a:b",
            "chat/:a(b)",
            "chat/:a)",
            "chat/:?",
            "chat/:a+",
            "chat/:a*",
            "chat/:",
            "chat/:a/:a",
            "*/after",
            "chat/a?b",
        ] {
            let config = json!({ "prefixes": [], "screens": { "S": pattern } });
            assert!(RouteTable::from_config(&config).is_err(), "{pattern}");
        }
    }

    #[test]
    fn one_pattern_cannot_map_to_two_screens() {
        let config = json!({
            "prefixes": [],
            "screens": { "One": "orders/:id", "Two": "orders/:id" }
        });
        assert_eq!(
            RouteTable::from_config(&config).err().code(),
            "ERR_DEEP_LINKS_INVALID_PARAMETER"
        );
    }
}
