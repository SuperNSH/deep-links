//! The files that make the web vouch for the app: Android's `assetlinks.json` and Apple's
//! `apple-app-site-association`, built and checked offline. Builders refuse bad input with
//! `ERR_DEEP_LINKS_INVALID_ARGUMENT`; validators refuse a file that is malformed or names
//! another app with `ERR_DEEP_LINKS_ASSOCIATION_INVALID`.
//!
//! Nothing here fetches. Validators take the bytes the caller already has: Apple's CDN caches a
//! failed fetch for days, so a library guessing at verification state at runtime would be wrong
//! for exactly as long.

use nsh_core::JsonValue;
use serde_json::json;

use crate::error::{DeepLinksError, ErrorKind};

/// The relation every Android statement declares.
const HANDLE_ALL_URLS: &str = "delegate_permission/common.handle_all_urls";
const ASSETLINKS_PATH: &str = ".well-known/assetlinks.json";
const APPLE_PATH: &str = ".well-known/apple-app-site-association";

/// One statement file and the address it is served at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AssociationFile {
    /// `https://<host>/.well-known/...`.
    pub url: String,
    /// The file's JSON, pretty-printed with two-space indents.
    pub body: String,
}

fn bad(detail: impl Into<String>) -> DeepLinksError {
    DeepLinksError::new(ErrorKind::InvalidArgument).with_detail(detail)
}

fn invalid(detail: impl Into<String>) -> DeepLinksError {
    DeepLinksError::new(ErrorKind::AssociationInvalid).with_detail(detail)
}

/// The `assetlinks.json` that vouches for `package`: one statement handing it every URL of the
/// host, with the SHA-256 fingerprints of its signing certificates, upper case and
/// colon-separated as Android writes them.
pub fn assetlinks_json(package: &str, fingerprints: &[&str]) -> Result<String, DeepLinksError> {
    if !android_package_is_valid(package) {
        return Err(bad(format!(
            "`{package}` is not an Android package name: two or more dot-separated words, each a letter then letters, digits or `_`"
        )));
    }
    if fingerprints.is_empty() {
        return Err(bad(
            "at least one SHA-256 certificate fingerprint is needed",
        ));
    }
    let mut normalized = Vec::with_capacity(fingerprints.len());
    for fingerprint in fingerprints {
        let hex = fingerprint_hex(fingerprint).ok_or_else(|| {
            bad(format!(
                "`{fingerprint}` is not a SHA-256 fingerprint: 64 hex digits, colons allowed"
            ))
        })?;
        normalized.push(colonated(&hex));
    }
    // Keys in alphabetical order, so the output is the same whichever map serde_json uses.
    pretty(&json!([{
        "relation": [HANDLE_ALL_URLS],
        "target": {
            "namespace": "android_app",
            "package_name": package,
            "sha256_cert_fingerprints": normalized,
        },
    }]))
}

/// One `assetlinks.json` per host: `autoVerify` checks every host of every intent filter, and
/// each subdomain serves its own file, so `example.com` and `www.example.com` both need one.
pub fn assetlinks_files(
    hosts: &[&str],
    package: &str,
    fingerprints: &[&str],
) -> Result<Vec<AssociationFile>, DeepLinksError> {
    let body = assetlinks_json(package, fingerprints)?;
    files(hosts, ASSETLINKS_PATH, &body)
}

/// The `apple-app-site-association` that vouches for `app_ids` (`TEAMID.bundle.id`) on the
/// paths `components` names (`/orders/*`); none means every path, `/*`.
pub fn apple_app_site_association(
    app_ids: &[&str],
    components: &[&str],
) -> Result<String, DeepLinksError> {
    if app_ids.is_empty() {
        return Err(bad("at least one app ID is needed: TEAMID.bundle.id"));
    }
    for app_id in app_ids {
        if !apple_app_id_is_valid(app_id) {
            return Err(bad(format!(
                "`{app_id}` is not an app ID: a 10-character team ID, a dot, the bundle ID"
            )));
        }
    }
    let components = if components.is_empty() {
        vec!["/*"]
    } else {
        components.to_vec()
    };
    let mut parsed = Vec::with_capacity(components.len());
    for component in components {
        if !component_is_valid(component) {
            return Err(bad(format!(
                "`{component}` is not a path component: it starts with `/` and has no whitespace"
            )));
        }
        parsed.push(json!({ "/": component }));
    }
    pretty(&json!({
        "applinks": {
            "details": [ { "appIDs": app_ids, "components": parsed } ]
        }
    }))
}

/// One `apple-app-site-association` per host: each subdomain in the entitlement's `applinks:`
/// list is fetched on its own.
pub fn apple_app_site_association_files(
    hosts: &[&str],
    app_ids: &[&str],
    components: &[&str],
) -> Result<Vec<AssociationFile>, DeepLinksError> {
    let body = apple_app_site_association(app_ids, components)?;
    files(hosts, APPLE_PATH, &body)
}

/// Checks an `assetlinks.json` for `package`: a non-empty list of well-formed statements, one of
/// which hands `package` the host's URLs. A statement for another package alone is
/// `ERR_DEEP_LINKS_ASSOCIATION_INVALID`.
pub fn validate_assetlinks(text: &str, package: &str) -> Result<(), DeepLinksError> {
    if !android_package_is_valid(package) {
        return Err(bad(format!("`{package}` is not an Android package name")));
    }
    let file = parse(text)?;
    let statements = file.as_array().ok_or_else(|| {
        invalid("the file is a list of statements: [{ \"relation\": ..., \"target\": ... }]")
    })?;
    if statements.is_empty() {
        return Err(invalid("the file has no statements"));
    }
    let mut names_it = false;
    for statement in statements {
        let fields = statement
            .as_object()
            .ok_or_else(|| invalid("a statement is an object"))?;
        let relation = fields
            .get("relation")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| invalid("a statement's \"relation\" is an array"))?;
        let target = fields
            .get("target")
            .and_then(JsonValue::as_object)
            .ok_or_else(|| invalid("a statement has a \"target\" object"))?;
        if target.get("namespace").and_then(JsonValue::as_str) != Some("android_app") {
            // A web target (another site) is a valid statement this check does not judge.
            continue;
        }
        let named = target
            .get("package_name")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| invalid("an android_app target has a \"package_name\" string"))?;
        let fingerprints = target
            .get("sha256_cert_fingerprints")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| {
                invalid("an android_app target has a \"sha256_cert_fingerprints\" array")
            })?;
        if fingerprints.is_empty() {
            return Err(invalid(
                "an android_app target lists at least one fingerprint",
            ));
        }
        for fingerprint in fingerprints {
            if fingerprint.as_str().and_then(fingerprint_hex).is_none() {
                return Err(invalid("a fingerprint is 64 hex digits, colon-separated"));
            }
        }
        if named == package && relation.iter().any(|r| r.as_str() == Some(HANDLE_ALL_URLS)) {
            names_it = true;
        }
    }
    if !names_it {
        return Err(invalid(format!(
            "no statement hands `{package}` the host's URLs ({HANDLE_ALL_URLS})"
        )));
    }
    Ok(())
}

/// Checks an `apple-app-site-association` for `app_id`: an `applinks` object whose details are
/// well formed, one of which lists `app_id` (`appIDs`, or the pre-iOS 13 `appID`). A file for
/// another team or bundle alone is `ERR_DEEP_LINKS_ASSOCIATION_INVALID`.
pub fn validate_apple_app_site_association(text: &str, app_id: &str) -> Result<(), DeepLinksError> {
    if !apple_app_id_is_valid(app_id) {
        return Err(bad(format!(
            "`{app_id}` is not an app ID: a 10-character team ID, a dot, the bundle ID"
        )));
    }
    let file = parse(text)?;
    let fields = file.as_object().ok_or_else(|| {
        invalid("the file is an object: { \"applinks\": { \"details\": [...] } }")
    })?;
    let details = fields
        .get("applinks")
        .and_then(JsonValue::as_object)
        .and_then(|applinks| applinks.get("details"))
        .and_then(JsonValue::as_array)
        .ok_or_else(|| invalid("the file has \"applinks\": { \"details\": [...] }"))?;
    let mut names_it = false;
    for detail in details {
        let detail = detail
            .as_object()
            .ok_or_else(|| invalid("a detail is an object"))?;
        let listed: Vec<&str> = if let Some(app_ids) = detail.get("appIDs") {
            let app_ids = app_ids
                .as_array()
                .filter(|ids| !ids.is_empty())
                .ok_or_else(|| invalid("\"appIDs\" is a non-empty array"))?;
            let components = detail
                .get("components")
                .and_then(JsonValue::as_array)
                .ok_or_else(|| invalid("a detail with \"appIDs\" has a \"components\" array"))?;
            for component in components {
                let path = component
                    .as_object()
                    .ok_or_else(|| invalid("a component is an object"))?
                    .get("/");
                if path.is_some_and(|path| !path.as_str().is_some_and(component_is_valid)) {
                    return Err(invalid("a component's \"/\" path starts with `/`"));
                }
            }
            app_ids
                .iter()
                .map(|id| id.as_str().ok_or_else(|| invalid("an app ID is a string")))
                .collect::<Result<_, _>>()?
        } else if let Some(app_id) = detail.get("appID") {
            let app_id = app_id
                .as_str()
                .ok_or_else(|| invalid("\"appID\" is a string"))?;
            let paths = detail
                .get("paths")
                .and_then(JsonValue::as_array)
                .ok_or_else(|| invalid("a detail with \"appID\" has a \"paths\" array"))?;
            if paths.iter().any(|p| p.as_str().is_none_or(str::is_empty)) {
                return Err(invalid("a path pattern is a non-empty string"));
            }
            vec![app_id]
        } else {
            return Err(invalid("a detail has \"appIDs\", or the older \"appID\""));
        };
        for listed in listed {
            if !apple_app_id_is_valid(listed) {
                return Err(invalid(format!(
                    "`{listed}` is not an app ID: a 10-character team ID, a dot, the bundle ID"
                )));
            }
            names_it |= listed == app_id;
        }
    }
    if !names_it {
        return Err(invalid(format!(
            "no detail lists `{app_id}`: the file is for another team or bundle"
        )));
    }
    Ok(())
}

fn files(hosts: &[&str], path: &str, body: &str) -> Result<Vec<AssociationFile>, DeepLinksError> {
    if hosts.is_empty() {
        return Err(bad("at least one host is needed: example.com"));
    }
    let mut out: Vec<AssociationFile> = Vec::with_capacity(hosts.len());
    for host in hosts {
        let labels: Vec<&str> = host.split('.').collect();
        let valid = labels.len() >= 2
            && labels.iter().all(|label| {
                !label.is_empty()
                    && label
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'-')
            });
        if !valid {
            return Err(bad(format!(
                "`{host}` is not a host that serves a file: `www.example.com`, no scheme, port, path or `*`"
            )));
        }
        let url = format!("https://{}/{path}", host.to_ascii_lowercase());
        if out.iter().any(|file| file.url == url) {
            return Err(bad(format!("`{host}` is listed twice")));
        }
        out.push(AssociationFile {
            url,
            body: body.to_owned(),
        });
    }
    Ok(out)
}

fn pretty(value: &JsonValue) -> Result<String, DeepLinksError> {
    serde_json::to_string_pretty(value).map_err(|_| DeepLinksError::new(ErrorKind::Backend))
}

fn parse(text: &str) -> Result<JsonValue, DeepLinksError> {
    serde_json::from_str(text).map_err(|_| invalid("the file is not JSON"))
}

fn component_is_valid(component: &str) -> bool {
    component.starts_with('/') && !component.contains(char::is_whitespace)
}

/// The fingerprint's 64 hex digits, upper case; `None` when it is not a SHA-256 fingerprint.
fn fingerprint_hex(fingerprint: &str) -> Option<String> {
    let hex: String = fingerprint.chars().filter(|c| *c != ':').collect();
    (hex.len() == 64 && hex.chars().all(|c| c.is_ascii_hexdigit())).then(|| hex.to_uppercase())
}

/// Upper-case hex with a colon between bytes, as Android writes a fingerprint.
fn colonated(hex: &str) -> String {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| String::from_utf8_lossy(pair).into_owned())
        .collect::<Vec<_>>()
        .join(":")
}

/// A package name: two or more dot-separated words, each a letter then letters, digits or `_`.
fn android_package_is_valid(package: &str) -> bool {
    let words: Vec<&str> = package.split('.').collect();
    words.len() >= 2
        && words.iter().all(|word| {
            let mut chars = word.chars();
            chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// An app ID: a 10-character team ID of upper-case letters and digits, a dot, then a bundle ID
/// of letters, digits, `-` and `.`.
fn apple_app_id_is_valid(app_id: &str) -> bool {
    let Some((team, bundle)) = app_id.split_once('.') else {
        return false;
    };
    team.len() == 10
        && team
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit())
        && !bundle.is_empty()
        && bundle.split('.').all(|word| {
            !word.is_empty() && word.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE: &str = "com.supernsh.shop";
    const APP_ID: &str = "1A2B3C4D5E.com.supernsh.shop";
    const FINGERPRINT: &str = "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99";

    fn code<T>(answer: Result<T, DeepLinksError>) -> Option<&'static str> {
        answer.err().map(|error| error.code())
    }

    #[test]
    fn the_android_builder_refuses_bad_input() {
        let lower = FINGERPRINT.to_ascii_lowercase();
        assert!(assetlinks_json(PACKAGE, &[FINGERPRINT, &lower]).is_ok());
        for (package, fingerprints) in [
            ("supernsh", &[FINGERPRINT] as &[&str]),
            ("", &[FINGERPRINT]),
            ("com.supernsh", &[]),
            ("com.supernsh", &["not hex"]),
            ("9com.supernsh", &[FINGERPRINT]),
        ] {
            assert_eq!(
                code(assetlinks_json(package, fingerprints)),
                Some("ERR_DEEP_LINKS_INVALID_ARGUMENT"),
                "{package}"
            );
        }
    }

    #[test]
    fn the_android_validator_refuses_a_malformed_file() {
        for file in [
            "not json",
            "{}",
            "[]",
            r#"[{ "target": {} }]"#,
            r#"[{ "relation": [], "target": { "namespace": "android_app", "package_name": "com.supernsh.shop", "sha256_cert_fingerprints": [] } }]"#,
            r#"[{ "relation": [], "target": { "namespace": "android_app", "package_name": "com.supernsh.shop", "sha256_cert_fingerprints": ["AA"] } }]"#,
            r#"[{ "relation": ["other"], "target": { "namespace": "android_app", "package_name": "com.supernsh.shop", "sha256_cert_fingerprints": ["AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99"] } }]"#,
        ] {
            assert_eq!(
                code(validate_assetlinks(file, PACKAGE)),
                Some("ERR_DEEP_LINKS_ASSOCIATION_INVALID"),
                "{file}"
            );
        }
    }

    #[test]
    fn the_apple_builder_refuses_bad_input() {
        for (app_ids, components) in [
            (&[] as &[&str], &[] as &[&str]),
            (&["SHORT.com.supernsh"], &[]),
            (&["1a2b3c4d5e.com.supernsh"], &[]),
            (&["1A2B3C4D5E."], &[]),
            (&[APP_ID], &["orders/*"]),
            (&[APP_ID], &["/a b"]),
        ] {
            assert_eq!(
                code(apple_app_site_association(app_ids, components)),
                Some("ERR_DEEP_LINKS_INVALID_ARGUMENT")
            );
        }
    }

    #[test]
    fn the_apple_validator_takes_both_formats() {
        let modern = format!(
            r#"{{"applinks": {{"details": [{{"appIDs": ["{APP_ID}"], "components": [{{"/": "/*"}}, {{"?": {{"q": "1"}}}}]}}]}}}}"#
        );
        assert!(validate_apple_app_site_association(&modern, APP_ID).is_ok());
        let legacy = format!(
            r#"{{"applinks": {{"apps": [], "details": [{{"appID": "{APP_ID}", "paths": ["*"]}}]}}}}"#
        );
        assert!(validate_apple_app_site_association(&legacy, APP_ID).is_ok());
        for file in [
            "not json",
            "[]",
            "{}",
            r#"{"applinks": {}}"#,
            r#"{"applinks": {"details": [{}]}}"#,
            r#"{"applinks": {"details": [{"appIDs": []}]}}"#,
            r#"{"applinks": {"details": [{"appID": "1A2B3C4D5E.com.supernsh.shop", "paths": [""]}]}}"#,
        ] {
            assert_eq!(
                code(validate_apple_app_site_association(file, APP_ID)),
                Some("ERR_DEEP_LINKS_ASSOCIATION_INVALID"),
                "{file}"
            );
        }
    }

    #[test]
    fn a_host_that_cannot_serve_a_file_is_refused() {
        for hosts in [
            &[] as &[&str],
            &["https://example.com"],
            &["a/b"],
            &[""],
            &["*.example.com"],
            &["example.com", "EXAMPLE.com"],
            &["example.com:443"],
        ] {
            assert_eq!(
                code(assetlinks_files(hosts, PACKAGE, &[FINGERPRINT])),
                Some("ERR_DEEP_LINKS_INVALID_ARGUMENT"),
                "{hosts:?}"
            );
        }
    }
}
