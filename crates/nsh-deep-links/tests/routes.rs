//! The route-table and config edge cases of docs/goals.md, named as goals.md names them.

use nsh_deep_links::{DeepLinksError, RouteMatch, RouteTable};
use serde_json::{Value, json};

fn table(config: &Value) -> RouteTable {
    RouteTable::from_config(config).unwrap_or_else(|error| panic!("{config} is valid: {error}"))
}

fn code<T>(answer: Result<T, DeepLinksError>) -> &'static str {
    match answer {
        Ok(_) => "no error",
        Err(error) => error.code(),
    }
}

fn matched(table: &RouteTable, url: &str) -> RouteMatch {
    table
        .match_url(url)
        .unwrap_or_else(|error| panic!("{url} routes: {error}"))
}

#[test]
fn more_specific_patterns_match_before_wildcards() {
    // Listed least specific first: the order in the config does not decide.
    let routes = table(&json!({
        "prefixes": ["myapp://"],
        "screens": {
            "Anything": "*",
            "Rest": "files/:rest*",
            "Many": "files/:path+",
            "Name": "files/:name",
            "Numbered": "files/:id(\\d+)",
            "New": "files/new"
        }
    }));
    for (url, screen) in [
        ("myapp://files/new", "New"),
        ("myapp://files/42", "Numbered"),
        ("myapp://files/report", "Name"),
        ("myapp://files/a/b", "Many"),
        ("myapp://files", "Rest"),
        ("myapp://other/x", "Anything"),
        ("myapp://", "Anything"),
    ] {
        assert_eq!(matched(&routes, url).screen, screen, "{url}");
    }
    assert_eq!(
        matched(&routes, "myapp://files/a/b").params.get("path"),
        Some(&json!("a/b"))
    );
}

#[test]
fn a_param_failing_its_parse_schema_rejects_as_invalid_parameter() {
    let routes = table(&json!({
        "prefixes": ["myapp://"],
        "screens": {
            "Order": { "path": "orders/:id", "parse": { "id": "integer", "gift": "boolean" } },
            "Price": { "path": "price/:amount", "parse": { "amount": "number" } },
            "Anything": "*"
        }
    }));
    for url in [
        "myapp://orders/abc",
        "myapp://orders/4.5",
        "myapp://orders/42?gift=yes",
        "myapp://price/1e400",
        "myapp://price/0x10",
    ] {
        // Never routed with a bad value, and never handed to a looser pattern instead.
        assert_eq!(
            code(routes.match_url(url)),
            "ERR_DEEP_LINKS_INVALID_PARAMETER",
            "{url}"
        );
    }
    let order = matched(&routes, "myapp://orders/42?gift=true");
    assert_eq!(order.params.get("id"), Some(&json!(42)));
    assert_eq!(order.params.get("gift"), Some(&json!(true)));
    assert_eq!(
        matched(&routes, "myapp://price/-2.5").params.get("amount"),
        Some(&json!(-2.5))
    );
}

#[test]
fn an_invalid_pattern_in_the_route_table_is_rejected_as_invalid_argument() {
    for pattern in [
        "orders/a:b",
        "orders/:id(",
        "orders/(x)",
        "orders/:id)",
        "orders/a?",
        "orders/:id?+",
        "orders/:id+?",
        "orders/:",
        "orders/:id/:id",
        "orders/:id([)",
    ] {
        let config = json!({ "prefixes": ["myapp://"], "screens": { "Order": pattern } });
        let error = RouteTable::from_config(&config).err();
        assert_eq!(
            error.as_ref().map(|e| e.code()),
            Some("ERR_DEEP_LINKS_INVALID_ARGUMENT"),
            "{pattern}"
        );
        let detail = error
            .and_then(|e| e.detail().map(str::to_owned))
            .unwrap_or_default();
        assert!(
            detail.contains(pattern),
            "the error names `{pattern}`: {detail}"
        );
    }
    for config in [
        json!({ "prefixes": ["myapp://"], "screens": { "Order": { "path": "o", "params": {} } } }),
        json!({ "prefixes": ["myapp://"], "path": "app/:tenant", "screens": {} }),
        json!({ "prefixes": ["myapp://"], "screens": { "Order": { "alias": ["o"] } } }),
        json!({ "prefixes": ["myapp://"], "linking": true }),
    ] {
        assert_eq!(
            code(RouteTable::from_config(&config)),
            "ERR_DEEP_LINKS_INVALID_ARGUMENT",
            "{config}"
        );
    }
}

#[test]
fn conflicting_patterns_for_two_screens_reject_as_invalid_argument() {
    let config = json!({
        "prefixes": ["myapp://"],
        "screens": { "One": "orders/:id", "Two": "orders//:id/" }
    });
    assert_eq!(
        code(RouteTable::from_config(&config)),
        "ERR_DEEP_LINKS_INVALID_ARGUMENT"
    );
    let nested = json!({
        "prefixes": ["myapp://"],
        "screens": {
            "Shop": { "path": "shop", "screens": { "Cart": "cart" } },
            "Cart2": "shop/cart"
        }
    });
    assert_eq!(
        code(RouteTable::from_config(&nested)),
        "ERR_DEEP_LINKS_INVALID_ARGUMENT"
    );
    // One screen claiming one pattern twice is not a conflict.
    let alias = json!({
        "prefixes": ["myapp://"],
        "screens": { "Order": { "path": "orders/:id", "alias": ["orders/:id", "o/:id"] } }
    });
    assert_eq!(matched(&table(&alias), "myapp://o/1").screen, "Order");
}

#[test]
fn a_reserved_scheme_in_a_claim_is_rejected_as_disallowed() {
    for scheme in [
        "mailto",
        "file",
        "javascript",
        "tel",
        "data",
        "nsh",
        "Intent",
    ] {
        let config = json!({ "prefixes": [format!("{scheme}://")], "screens": {} });
        assert_eq!(
            code(RouteTable::from_config(&config)),
            "ERR_DEEP_LINKS_DISALLOWED_SCHEME",
            "{scheme}"
        );
    }
    // A scheme that is not of URL-scheme form is a config error, not a reserved one.
    let malformed = json!({ "prefixes": ["1app://"], "screens": {} });
    assert_eq!(
        code(RouteTable::from_config(&malformed)),
        "ERR_DEEP_LINKS_INVALID_ARGUMENT"
    );
    let fine = table(&json!({ "prefixes": ["MyApp://", "https://example.com", "myapp://deep"] }));
    assert_eq!(fine.claimed_schemes(), vec!["myapp".to_owned()]);
}
