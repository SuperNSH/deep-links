//! The association-file edge cases of docs/goals.md, named as goals.md names them. The exact JSON
//! below is asserted by the TypeScript suite too.

use nsh_deep_links::{
    DeepLinksError,
    association::{
        AssociationFile, apple_app_site_association, apple_app_site_association_files,
        assetlinks_files, assetlinks_json, validate_apple_app_site_association,
        validate_assetlinks,
    },
};

const PACKAGE: &str = "com.supernsh.shop";
const APP_ID: &str = "1A2B3C4D5E.com.supernsh.shop";
const FINGERPRINT: &str = "aa:bb:cc:dd:ee:ff:00:11:22:33:44:55:66:77:88:99:aa:bb:cc:dd:ee:ff:00:11:22:33:44:55:66:77:88:99";

const ASSETLINKS: &str = r#"[
  {
    "relation": [
      "delegate_permission/common.handle_all_urls"
    ],
    "target": {
      "namespace": "android_app",
      "package_name": "com.supernsh.shop",
      "sha256_cert_fingerprints": [
        "AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99"
      ]
    }
  }
]"#;

const APPLE: &str = r#"{
  "applinks": {
    "details": [
      {
        "appIDs": [
          "1A2B3C4D5E.com.supernsh.shop"
        ],
        "components": [
          {
            "/": "/orders/*"
          }
        ]
      }
    ]
  }
}"#;

fn code<T>(answer: Result<T, DeepLinksError>) -> &'static str {
    match answer {
        Ok(_) => "no error",
        Err(error) => error.code(),
    }
}

fn ok<T>(answer: Result<T, DeepLinksError>) -> T {
    answer.unwrap_or_else(|error| panic!("expected a value, got {error}"))
}

#[test]
fn an_apple_association_for_the_wrong_team_is_invalid() {
    let file = ok(apple_app_site_association(&[APP_ID], &["/orders/*"]));
    assert_eq!(file, APPLE);
    assert!(validate_apple_app_site_association(&file, APP_ID).is_ok());
    for expected in [
        "9Z8Y7X6W5V.com.supernsh.shop",
        "1A2B3C4D5E.com.supernsh.other",
    ] {
        assert_eq!(
            code(validate_apple_app_site_association(&file, expected)),
            "ERR_DEEP_LINKS_ASSOCIATION_INVALID",
            "{expected}"
        );
    }
    // The pre-iOS 13 form names the team the same way.
    let legacy = r#"{"applinks": {"apps": [], "details": [{"appID": "9Z8Y7X6W5V.com.supernsh.shop", "paths": ["*"]}]}}"#;
    assert_eq!(
        code(validate_apple_app_site_association(legacy, APP_ID)),
        "ERR_DEEP_LINKS_ASSOCIATION_INVALID"
    );
    // A malformed shape is invalid too, and a bad expected app ID is the caller's error.
    assert_eq!(
        code(validate_apple_app_site_association("{}", APP_ID)),
        "ERR_DEEP_LINKS_ASSOCIATION_INVALID"
    );
    assert_eq!(
        code(validate_apple_app_site_association(&file, "shop")),
        "ERR_DEEP_LINKS_INVALID_ARGUMENT"
    );
    assert_eq!(
        code(apple_app_site_association(&[""], &[])),
        "ERR_DEEP_LINKS_INVALID_ARGUMENT"
    );
}

#[test]
fn an_assetlinks_statement_for_the_wrong_package_is_invalid() {
    let file = ok(assetlinks_json(PACKAGE, &[FINGERPRINT]));
    assert_eq!(file, ASSETLINKS);
    assert!(validate_assetlinks(&file, PACKAGE).is_ok());
    assert_eq!(
        code(validate_assetlinks(&file, "com.supernsh.other")),
        "ERR_DEEP_LINKS_ASSOCIATION_INVALID"
    );
    // A statement that misplaces the package (outside `target`) does not name it.
    let misplaced = r#"[{"relation": ["delegate_permission/common.handle_all_urls"], "package_name": "com.supernsh.shop", "target": {"namespace": "android_app", "package_name": "com.supernsh.other", "sha256_cert_fingerprints": ["AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99:AA:BB:CC:DD:EE:FF:00:11:22:33:44:55:66:77:88:99"]}}]"#;
    assert_eq!(
        code(validate_assetlinks(misplaced, PACKAGE)),
        "ERR_DEEP_LINKS_ASSOCIATION_INVALID"
    );
    assert_eq!(
        code(assetlinks_json("", &[FINGERPRINT])),
        "ERR_DEEP_LINKS_INVALID_ARGUMENT"
    );
}

#[test]
fn assetlinks_statements_are_built_per_subdomain() {
    let files = ok(assetlinks_files(
        &["example.com", "www.example.com", "Shop.Example.com"],
        PACKAGE,
        &[FINGERPRINT],
    ));
    let urls: Vec<&str> = files.iter().map(|file| file.url.as_str()).collect();
    assert_eq!(
        urls,
        [
            "https://example.com/.well-known/assetlinks.json",
            "https://www.example.com/.well-known/assetlinks.json",
            "https://shop.example.com/.well-known/assetlinks.json",
        ]
    );
    assert!(files.iter().all(|file| file.body == ASSETLINKS));
    let apple = ok(apple_app_site_association_files(
        &["example.com"],
        &[APP_ID],
        &["/orders/*"],
    ));
    assert_eq!(
        apple,
        vec![AssociationFile {
            url: "https://example.com/.well-known/apple-app-site-association".to_owned(),
            body: APPLE.to_owned(),
        }]
    );
    // A wildcard is not a host that can serve a file, and a host listed twice is a mistake.
    for hosts in [
        &["*.example.com"][..],
        &["example.com", "EXAMPLE.COM"][..],
        &[][..],
    ] {
        assert_eq!(
            code(assetlinks_files(hosts, PACKAGE, &[FINGERPRINT])),
            "ERR_DEEP_LINKS_INVALID_ARGUMENT",
            "{hosts:?}"
        );
    }
}

#[test]
fn association_validation_is_offline_and_never_fetches() {
    // The validators take the bytes the caller has; there is no URL or client to give them.
    // The files' hosts are unroutable (`.invalid`, RFC 6761), so any fetch could not succeed,
    // and the answers below come back the same whatever the network does.
    let files = ok(assetlinks_files(&["shop.invalid"], PACKAGE, &[FINGERPRINT]));
    let checked: fn(&str, &str) -> Result<(), DeepLinksError> = validate_assetlinks;
    assert!(checked(&files[0].body, PACKAGE).is_ok());
    // A stale or failed CDN answer is just bytes: judged as such, never refreshed or guessed.
    for cached in ["", "<html>Not Found</html>", "{\"error\": \"timeout\"}"] {
        assert_eq!(
            code(validate_assetlinks(cached, PACKAGE)),
            "ERR_DEEP_LINKS_ASSOCIATION_INVALID"
        );
        assert_eq!(
            code(validate_apple_app_site_association(cached, APP_ID)),
            "ERR_DEEP_LINKS_ASSOCIATION_INVALID"
        );
    }
    let checked_apple: fn(&str, &str) -> Result<(), DeepLinksError> =
        validate_apple_app_site_association;
    let apple = ok(apple_app_site_association_files(
        &["shop.invalid"],
        &[APP_ID],
        &[],
    ));
    assert!(checked_apple(&apple[0].body, APP_ID).is_ok());
}
