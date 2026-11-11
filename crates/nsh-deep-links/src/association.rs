//! The association files that make the web vouch for the app: Android's `assetlinks.json` and
//! Apple's `apple-app-site-association`, built and checked offline. Builders refuse bad input
//! with `ERR_DEEP_LINKS_INVALID_ARGUMENT`; validators refuse bad files with
//! `ERR_DEEP_LINKS_ASSOCIATION_INVALID`. What the verifier fetches is the platform's business;
//! these never touch the network.

use nsh_core::JsonValue;
use serde_json::json;

use crate::error::{DeepLinksError, ErrorKind};

/// The relation every Android statement declares.
const HANDLE_ALL_URLS: &str = "delegate_permission/common.handle_all_urls";

/// Builds the file that vouches for `package` at
/// `https://<host>/.well-known/assetlinks.json`: one statement, one target, the SHA-256
/// fingerprints of the signing certificates, upper-case and colon-separated as Android writes
/// them.
pub fn assetlinks_json(package: &str, fingerprints: &[&str]) -> Result<String, DeepLinksError> {
    let bad = |what: &str| DeepLinksError::new(ErrorKind::InvalidArgument).with_detail(what);
    if !android_package_is_valid(package) {
        return Err(bad(&format!(
            "`{package}` is not an Android package name: words of a letter then letters, digits or `_`, two at least, dot-separated"
        )));
    }
    if fingerprints.is_empty() {
        return Err(bad("at least one SHA-256 certificate fingerprint is needed"));
    }
    let mut normalized = Vec::with_capacity(fingerprints.len());
    for fingerprint in fingerprints {
        let Some(hex) = fingerprint_hex(fingerprint) else {
            return Err(bad(&format!(
                "`{fingerprint}` is not a SHA-256 fingerprint: 64 hex digits, colons allowed"
            )));
        };
        normalized.push(colonated(&hex));
    }
    let statement = json!([{
        "relation": [HANDLE_ALL_URLS],
        "target": {
            "namespace": "android_app",
            "package_name": package,
            "sha256_cert_fingerprints": normalized,
        },
    }]);
    serde_json::to_string_pretty(&statement)
        .map_err(|_| DeepLinksError::new(ErrorKind::Backend))
}

/// Checks an `assetlinks.json`: a non-empty list of statements, each handing the app the URLs
/// of an `android_app` target with a package name and fingerprints. The wrong package, an
/// empty fingerprint list or a misplaced field is a coded failure, not a warning.
pub fn validate_assetlinks(text: &str) -> Result<(), DeepLinksError> {
    let invalid = |what: &str| {
        DeepLinksError::new(ErrorKind::AssociationInvalid).with_detail(what)
    };
    let file = parse(text)?;
    let Some(statements) = file.as_array() else {
        return Err(invalid("the file is a list of statements: [ { \"relation\": ..., \"target\": ... } ]"));
    };
    if statements.is_empty() {
        return Err(invalid("the file has no statements"));
    }
    for statement in statements {
        let fields = statement
            .as_object()
            .ok_or_else(|| invalid("a statement is an object"))?;
        let relation = fields
            .get("relation")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| invalid("a statement's \"relation\" is an array"))?;
        if !relation
            .iter()
            .any(|r| r.as_str() == Some(HANDLE_ALL_URLS))
        {
            return Err(invalid(&format!(
                "a statement's relation must include {HANDLE_ALL_URLS}"
            )));
        }
        let target = fields
            .get("target")
            .and_then(JsonValue::as_object)
            .ok_or_else(|| invalid("a statement has a \"target\" object"))?;
        if target.get("namespace").and_then(JsonValue::as_str) != Some("android_app") {
            return Err(invalid("a target's namespace is \"android_app\""));
        }
        let package = target
            .get("package_name")
            .and_then(JsonValue::as_str)
            .ok_or_else(|| invalid("a target has a \"package_name\" string"))?;
        if !android_package_is_valid(package) {
            return Err(invalid("a target's package_name is not an Android package name"));
        }
        let fingerprints = target
            .get("sha256_cert_fingerprints")
            .and_then(JsonValue::as_array)
            .ok_or_else(|| invalid("a target has a \"sha256_cert_fingerprints\" array"))?;
        if fingerprints.is_empty() {
            return Err(invalid("a target lists at least one fingerprint"));
        }
        for fingerprint in fingerprints {
            let fingerprint = fingerprint.as_str().ok_or_else(|| {
                invalid("a fingerprint is a string of 64 hex digits")
            })?;
            if fingerprint_hex(fingerprint).is_none() {
                return Err(invalid("a fingerprint is a string of 64 hex digits"));
            }
        }
    }
    Ok(())
}

/// Builds the file that vouches for the app IDs at
/// `https://<host>/.well-known/apple-app-site-association`: one detail, the team's app IDs, and
/// the path components. No components means every path: `["/*"]`.
pub fn apple_app_site_association(
    app_ids: &[&str],
    components: &[&str],
) -> Result<String, DeepLinksError> {
    let bad = |what: &str| DeepLinksError::new(ErrorKind::InvalidArgument).with_detail(what);
    if app_ids.is_empty() {
        return Err(bad("at least one app ID is needed: TEAMID.bundle.id"));
    }
    for app_id in app_ids {
        if !apple_app_id_is_valid(app_id) {
            return Err(bad(&format!(
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
    for component in &components {
        if !component.starts_with('/') || component.contains(char::is_whitespace) {
            return Err(bad(&format!(
                "`{component}` is a path component: it starts with `/`"
            )));
        }
        parsed.push(json!({ "/": component }));
    }
    let file = json!({
        "applinks": {
            "details": [ { "appIDs": app_ids, "components": parsed } ]
        }
    });
    serde_json::to_string_pretty(&file).map_err(|_| DeepLinksError::new(ErrorKind::Backend))
}

/// Checks an `apple-app-site-association`: the `applinks` key with details, each carrying app
/// IDs (`appIDs`, or the older singular `appID`) and well-formed path components. Other
/// top-level keys (`webcredentials`, ...) serve other features and are not judged here.
pub fn validate_apple_app_site_association(text: &str) -> Result<(), DeepLinksError> {
    let invalid = |what: &str| {
        DeepLinksError::new(ErrorKind::AssociationInvalid).with_detail(what)
    };
    let file = parse(text)?;
    let Some(fields) = file.as_object() else {
        return Err(invalid("the file is an object: { \"applinks\": { \"details\": [...] } }"));
    };
    if let Some(apps) = fields.get("apps") {
        // The legacy format's marker: present, it must be empty.
        if apps.as_array().is_none_or(|apps| !apps.is_empty()) {
            return Err(invalid(
                "\"apps\", when the old format has it, is an empty array",
            ));
        }
    }
    let applinks = fields
        .get("applinks")
        .and_then(JsonValue::as_object)
        .ok_or_else(|| invalid("the file has an \"applinks\" object"))?;
    let details = applinks
        .get("details")
        .and_then(JsonValue::as_array)
        .ok_or_else(|| invalid("\"applinks\" has a \"details\" array"))?;
    for detail in details {
        let detail = detail
            .as_object()
            .ok_or_else(|| invalid("a detail is an object"))?;
        if let Some(app_ids) = detail.get("appIDs") {
            let app_ids = app_ids
                .as_array()
                .ok_or_else(|| invalid("\"appIDs\" is an array of app IDs"))?;
            if app_ids.is_empty() {
                return Err(invalid("a detail lists at least one app ID"));
            }
            for app_id in app_ids {
                let app_id = app_id
                    .as_str()
                    .ok_or_else(|| invalid("an app ID is a string: TEAMID.bundle.id"))?;
                if !apple_app_id_is_valid(app_id) {
                    return Err(invalid(&format!(
                        "`{app_id}` is not an app ID: a 10-character team ID, a dot, the bundle ID"
                    )));
                }
            }
            let components = detail
                .get("components")
                .and_then(JsonValue::as_array)
                .ok_or_else(|| invalid("a detail has a \"components\" array"))?;
            for component in components {
                let path = component
                    .as_object()
                    .and_then(|c| c.get("/"))
                    .and_then(JsonValue::as_str)
                    .ok_or_else(|| invalid("a component is an object with a \"/\" path"))?;
                if !path.starts_with('/') || path.contains(char::is_whitespace) {
                    return Err(invalid("a component's path starts with `/`"));
                }
            }
        } else if let Some(app_id) = detail.get("appID") {
            // The format before iOS 13: one app ID as a string, paths as patterns.
            let app_id = app_id
                .as_str()
                .ok_or_else(|| invalid("\"appID\" is a string: TEAMID.bundle.id"))?;
            if !apple_app_id_is_valid(app_id) {
                return Err(invalid("`appID` is a 10-character team ID, a dot, the bundle ID"));
            }
            if let Some(paths) = detail.get("paths") {
                let paths = paths
                    .as_array()
                    .ok_or_else(|| invalid("\"paths\" is an array of path patterns"))?;
                for path in paths {
                    if path.as_str().is_none_or(str::is_empty) {
                        return Err(invalid("a path pattern is a non-empty string"));
                    }
                }
            }
        } else {
            return Err(invalid(
                "a detail has \"appIDs\", or the older \"appID\"",
            ));
        }
    }
    Ok(())
}

/// The `assetlinks.json` address of every host: each subdomain serves its own file, so
/// `www.example.com` and `example.com` each get one.
pub fn assetlinks_urls(hosts: &[&str]) -> Result<Vec<String>, DeepLinksError> {
    hosts
        .iter()
        .map(|host| statement_url(host, ".well-known/assetlinks.json"))
        .collect()
}

/// The `apple-app-site-association` address of every host: each subdomain serves its own file.
pub fn apple_association_urls(hosts: &[&str]) -> Result<Vec<String>, DeepLinksError> {
    hosts
        .iter()
        .map(|host| statement_url(host, ".well-known/apple-app-site-association"))
        .collect()
}

/// The `https` address of a statement file on one host, refusing a host that carries a scheme
/// or a path of its own.
fn statement_url(host: &str, file: &str) -> Result<String, DeepLinksError> {
    let bad = |what: &str| DeepLinksError::new(ErrorKind::InvalidArgument).with_detail(what);
    if host.is_empty()
        || host.contains(['/', ':', '?', '#'])
        || host.contains(char::is_whitespace)
    {
        return Err(bad(&format!(
            "`{host}` is a host: `www.example.com`, with no scheme or path"
        )));
    }
    Ok(format!("https://{host}/{file}"))
}

fn parse(text: &str) -> Result<JsonValue, DeepLinksError> {
    serde_json::from_str(text).map_err(|_| {
        DeepLinksError::new(ErrorKind::AssociationInvalid).with_detail("the file is not JSON")
    })
}

/// `Some(hex)` when the fingerprint is 64 hex digits, colons allowed: the SHA-256 of a
/// certificate.
fn fingerprint_hex(fingerprint: &str) -> Option<String> {
    let hex: String = fingerprint.chars().filter(|c| *c != ':').collect();
    let digits = 32 * 2;
    (hex.len() == digits && hex.chars().all(|c| c.is_ascii_hexdigit()))
        .then_some(hex.to_uppercase())
}

/// The fingerprint as Android writes it: upper-case, a colon between bytes.
fn colonated(hex: &str) -> String {
    hex.as_bytes()
        .chunks(2)
        .map(|pair| std::str::from_utf8(pair).unwrap_or_default())
        .collect::<Vec<_>>()
        .join(":")
}

/// A Play Store package name: at least two words of a letter then letters, digits or `_`.
fn android_package_is_valid(package: &str) -> bool {
    let words: Vec<&str> = package.split('.').collect();
    words.len() >= 2
        && words.iter().all(|word| {
            let mut chars = word.chars();
            chars.next().is_some_and(|c| c.is_ascii_alphabetic())
                && chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
        })
}

/// An Apple app ID: a 10-character team ID, a dot, then the bundle ID's words.
fn apple_app_id_is_valid(app_id: &str) -> bool {
    let words: Vec<&str> = app_id.split('.').collect();
    words.len() >= 2
        && words[0].len() == 10
        && words[0].chars().all(|c| c.is_ascii_uppercase() || c.is_ascii_digit())
        && android_package_is_valid(&words[1..].join("."))
}

#[cfg(test)]
mod tests {
    use super::*;

    const PACKAGE: &str = "com.supernsh.shop";
    const TEAM: &str = "1A2B3C4D5E";
    const FINGERPRINT: &str = "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99";

    #[test]
    fn the_android_file_builds_what_the_validator_takes() {
        let file = assetlinks_json(PACKAGE, &[FINGERPRINT, FINGERPRINT.to_ascii_lowercase()])
            .ok()
            .expect("a valid build");
        assert!(validate_assetlinks(&file).is_ok());
        // The fingerprint is normalized: lower-case input still comes out colonated.
        assert!(file.contains("AA:BB:CC:DD"));
    }

    #[test]
    fn the_android_builder_refuses_what_the_validator_would() {
        for (package, fingerprints) in [
            ("supernsh", &[FINGERPRINT] as &[&str]),
            ("", &[FINGERPRINT]),
            ("com.supernsh", &[]),
            ("com.supernsh", &["not hex"]),
            ("com.supernsh", &["AA"]),
            ("9com.supernsh", &[FINGERPRINT]),
        ] {
            let error = assetlinks_json(package, fingerprints).err();
            assert_eq!(
                error.map(DeepLinksError::code),
                Some("ERR_DEEP_LINKS_INVALID_ARGUMENT")
            );
        }
    }

    #[test]
    fn the_android_validator_catches_a_wrong_or_misplaced_field() {
        for file in [
            "not json",
            "{}",
            "[]",
            r#"[{ "relation": ["other"], "target": { "namespace": "android_app", "package_name": "com.supernsh", "sha256_cert_fingerprints": ["AA"] } }]"#,
            r#"[{ "relation": ["delegate_permission/common.handle_all_urls"], "target": { "namespace": "web", "package_name": "com.supernsh", "sha256_cert_fingerprints": ["AA:BB"] } }]"#,
            r#"[{ "relation": ["delegate_permission/common.handle_all_urls"], "target": { "namespace": "android_app", "package_name": "supernsh", "sha256_cert_fingerprints": ["AA:BB"] } }]"#,
            r#"[{ "relation": ["delegate_permission/common.handle_all_urls"], "target": { "namespace": "android_app", "package_name": "com.supernsh", "sha256_cert_fingerprints": [] } }]"#,
            r#"[{ "relation": ["delegate_permission/common.handle_all_urls"], "target": { "namespace": "android_app", "sha256_cert_fingerprints": ["AA"] } }]"#,
            r#"[{ "relation": ["delegate_permission/common.handle_all_urls"] }]"#,
        ] {
            let error = validate_assetlinks(file).err();
            assert_eq!(
                error.as_ref().map(DeepLinksError::code),
                Some("ERR_DEEP_LINKS_ASSOCIATION_INVALID"),
                "{file}"
            );
        }
    }

    #[test]
    fn the_apple_file_builds_what_the_validator_takes() {
        let file = apple_app_site_association(&[&format!("{TEAM}.{PACKAGE}")], &["/orders/*"])
            .ok()
            .expect("a valid build");
        assert!(validate_apple_app_site_association(&file).is_ok());
        assert!(file.contains("\"/\": \"/orders/*\""));

        // No components means every path.
        let all = apple_app_site_association(&[&format!("{TEAM}.{PACKAGE}")], &[])
            .ok()
            .expect("a valid build");
        assert!(all.contains("\"/*\""));
        assert!(validate_apple_app_site_association(&all).is_ok());
    }

    #[test]
    fn the_apple_builder_refuses_a_wrong_app_id_or_component() {
        for (app_ids, components) in [
            (&[] as &[&str], &[] as &[&str]),
            (&["SHORT.com.supernsh"], &[]),
            (&["1a2b3c4d5e.com.supernsh"], &[]), // team IDs are upper-case
            (&["1A2B3C4D5E.supernsh"], &[]),
            (&["1A2B3C4D5E.com.supernsh"], &["orders/*"]),
            (&["1A2B3C4D5E.com.supernsh"], &["/a b"]),
        ] {
            let error = apple_app_site_association(app_ids, components).err();
            assert_eq!(
                error.map(DeepLinksError::code),
                Some("ERR_DEEP_LINKS_INVALID_ARGUMENT")
            );
        }
    }

    #[test]
    fn the_apple_validator_takes_both_formats_and_refuses_a_wrong_one() {
        let modern = format!(
            r#"{{"applinks": {{"details": [{{"appIDs": ["{TEAM}.{PACKAGE}"], "components": [{{"/": "/*"}}]}}]}}}}"#
        );
        assert!(validate_apple_app_site_association(&modern).is_ok());
        let legacy = format!(
            r#"{{"apps": [], "applinks": {{"details": [{{"appID": "{TEAM}.{PACKAGE}", "paths": ["*"]}}]}}}}"#
        );
        assert!(validate_apple_app_site_association(&legacy).is_ok());
        // A file with other features' keys beside applinks is still a valid file.
        let shared = format!(
            r#"{{"webcredentials": {{"apps": ["{TEAM}.{PACKAGE}"]}}, "applinks": {{"details": [{{"appIDs": ["{TEAM}.{PACKAGE}"], "components": [{{"/": "/*"}}]}}]}}}}"#
        );
        assert!(validate_apple_app_site_association(&shared).is_ok());

        for file in [
            "not json",
            "[]",
            "{}",
            r#"{"applinks": {}}"#,
            r#"{"applinks": {"details": [{}]}}"#,
            r#"{"applinks": {"details": [{"appIDs": []}]}}"#,
            r#"{"applinks": {"details": [{"appIDs": ["SHORT.com.supernsh"]}]}}"#,
            r#"{"applinks": {"details": [{"appIDs": ["1A2B3C4D5E.com.supernsh"], "components": [{"?": "q"}]}]}}"#,
            r#"{"apps": ["full"], "applinks": {"details": []}}"#,
            r#"{"applinks": {"details": [{"appID": "1A2B3C4D5E.com.supernsh", "paths": [""]}]}}"#,
        ] {
            let error = validate_apple_app_site_association(file).err();
            assert_eq!(
                error.as_ref().map(DeepLinksError::code),
                Some("ERR_DEEP_LINKS_ASSOCIATION_INVALID"),
                "{file}"
            );
        }
    }

    #[test]
    fn every_subdomain_gets_its_own_statement_address() {
        let android =
            assetlinks_urls(&["example.com", "www.example.com"]).ok().expect("hosts");
        assert_eq!(
            android,
            vec![
                "https://example.com/.well-known/assetlinks.json".to_owned(),
                "https://www.example.com/.well-known/assetlinks.json".to_owned(),
            ]
        );
        let apple = apple_association_urls(&["example.com"]).ok().expect("hosts");
        assert_eq!(
            apple,
            vec![
                "https://example.com/.well-known/apple-app-site-association".to_owned()
            ]
        );
        for hosts in [&["https://example.com" as &str][..], &["a/b"][..], &[""][..]] {
            assert!(assetlinks_urls(hosts).is_err());
            assert!(apple_association_urls(hosts).is_err());
        }
    }
}
