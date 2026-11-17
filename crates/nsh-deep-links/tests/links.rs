//! The delivery and pipeline edge cases of docs/goals.md, one test each, named as goals.md names
//! them, driven through the public API and `FakeBackend` on tokio's paused (manual) clock.

use std::time::Duration;

use nsh_deep_links::{
    DeepLinks, DeepLinksError, ErrorKind, FakeBackend, Link, LinkKind, MAX_PENDING, MAX_URL_LEN,
    RouteTable, platform::unsupported::UnsupportedBackend,
};
use serde_json::json;

fn routes() -> RouteTable {
    let config = json!({
        "prefixes": ["https://example.com", "myapp://"],
        "screens": {
            "Home": "",
            "Order": { "path": "orders/:id", "parse": { "id": "integer" } },
            "Chat": "chat/:author",
            "Login": "auth/callback",
            "Cafe": "caf%C3%A9"
        }
    });
    match RouteTable::from_config(&config) {
        Ok(routes) => routes,
        Err(error) => panic!("the fixture config is valid: {error}"),
    }
}

fn links(fake: &FakeBackend) -> DeepLinks {
    DeepLinks::new(fake.clone(), routes())
}

fn verified(kind: LinkKind) -> (FakeBackend, DeepLinks) {
    let fake = FakeBackend::new();
    fake.set_web_link_kind(Some(kind));
    let links = links(&fake);
    (fake, links)
}

/// Fails, instead of hanging, when `future` never resolves: on the paused clock an idle
/// runtime jumps straight to the bound.
async fn bounded<F: std::future::Future>(future: F) -> F::Output {
    match tokio::time::timeout(Duration::from_secs(600), future).await {
        Ok(answer) => answer,
        Err(_) => panic!("the call never resolved"),
    }
}

/// Lets the other branch of a `join!` start its wait first.
async fn settle() {
    for _ in 0..4 {
        tokio::task::yield_now().await;
    }
}

fn code<T>(answer: Result<T, DeepLinksError>) -> &'static str {
    match answer {
        Ok(_) => "no error",
        Err(error) => error.code(),
    }
}

fn ok<T>(answer: Result<T, DeepLinksError>) -> T {
    answer.unwrap_or_else(|error| panic!("expected a value, got {error}"))
}

/// The link a delivered URL resolves with, through `next`.
async fn delivered(
    fake: &FakeBackend,
    links: &DeepLinks,
    url: &str,
) -> Result<Link, DeepLinksError> {
    let url = url.to_owned();
    let (answer, ()) = bounded(async {
        tokio::join!(links.next(), async {
            settle().await;
            ok(fake.deliver(&url));
        })
    })
    .await;
    answer
}

#[tokio::test(start_paused = true)]
async fn a_cold_start_link_dispatches_exactly_once() {
    let fake = FakeBackend::new();
    fake.set_initial(Some("myapp://orders/42"));
    let links = links(&fake);
    let first = ok(links.initial().await).unwrap_or_else(|| panic!("a launch link"));
    assert_eq!(first.kind, LinkKind::Custom);
    assert_eq!(first.route.screen, "Order");
    assert_eq!(first.route.params.get("id"), Some(&json!(42)));
    // Read again, it is the same answer, and the platform was asked once.
    assert_eq!(ok(links.initial().await), Some(first));
    assert_eq!(fake.initial_reads(), 1);
    // `next` never hands the launch link out again.
    assert_eq!(
        code(links.next_within(Duration::from_secs(5)).await),
        "ERR_DEEP_LINKS_TIMEOUT"
    );
}

#[tokio::test(start_paused = true)]
async fn initial_is_none_when_no_link_started_the_app() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    assert_eq!(ok(links.initial().await), None);
    assert_eq!(ok(links.initial().await), None);
}

#[tokio::test(start_paused = true)]
async fn a_link_while_running_dispatches_through_next() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    let link = ok(delivered(&fake, &links, "myapp://chat/jane").await);
    assert_eq!(link.url, "myapp://chat/jane");
    assert_eq!(link.route.screen, "Chat");
    assert_eq!(fake.deliveries(), 1);
}

#[tokio::test(start_paused = true)]
async fn stop_cancels_a_pending_next_exactly_once() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    let (answer, stopped) = bounded(async {
        tokio::join!(links.next(), async {
            settle().await;
            let stopped = links.stop().await;
            ok(fake.deliver("myapp://chat/late"));
            stopped
        })
    })
    .await;
    assert_eq!(code(answer), "ERR_DEEP_LINKS_CANCELLED");
    assert!(ok(stopped));
    // The late link never reaches a caller: delivery has ended.
    assert_eq!(
        code(bounded(links.next()).await),
        "ERR_DEEP_LINKS_CANCELLED"
    );
    assert_eq!(fake.queued(), vec!["myapp://chat/late".to_owned()]);
    assert!(!ok(links.stop().await));
}

#[tokio::test(start_paused = true)]
async fn a_next_waiting_past_its_deadline_resolves_as_timeout() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    let started = tokio::time::Instant::now();
    assert_eq!(
        code(links.next_within(Duration::from_secs(30)).await),
        "ERR_DEEP_LINKS_TIMEOUT"
    );
    assert_eq!(started.elapsed(), Duration::from_secs(30));
    // The gate is free, and a link that comes later is not lost.
    assert!(!fake.waiting());
    ok(fake.deliver("myapp://chat/jane"));
    assert_eq!(ok(bounded(links.next()).await).route.screen, "Chat");
    assert_eq!(
        code(links.next_within(Duration::ZERO).await),
        "ERR_DEEP_LINKS_INVALID_ARGUMENT"
    );
}

#[tokio::test(start_paused = true)]
async fn a_duplicate_delivery_while_one_resolves_maps_to_busy() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    let (answer, duplicate) = bounded(async {
        tokio::join!(links.next(), async {
            settle().await;
            ok(fake.deliver("myapp://chat/jane"));
            fake.deliver("myapp://chat/jane")
        })
    })
    .await;
    assert_eq!(code(duplicate), "ERR_DEEP_LINKS_BUSY");
    assert_eq!(ok(answer).url, "myapp://chat/jane");
    // The duplicate was not held for a later wait.
    assert_eq!(
        code(links.next_within(Duration::from_secs(1)).await),
        "ERR_DEEP_LINKS_TIMEOUT"
    );
}

#[tokio::test(start_paused = true)]
async fn a_second_next_while_one_is_outstanding_is_busy() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    let (first, second) = bounded(async {
        tokio::join!(links.next(), async {
            settle().await;
            let second = bounded(links.next()).await;
            ok(fake.deliver("myapp://chat/jane"));
            second
        })
    })
    .await;
    assert_eq!(code(second), "ERR_DEEP_LINKS_BUSY");
    assert_eq!(ok(first).route.screen, "Chat");
}

#[tokio::test(start_paused = true)]
async fn a_malformed_url_is_rejected_as_invalid_url() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    for url in ["", "/relative", "orders/42", "myapp", "://orders"] {
        assert_eq!(
            code(delivered(&fake, &links, url).await),
            "ERR_DEEP_LINKS_INVALID_URL",
            "{url:?}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn a_url_longer_than_max_url_len_is_rejected_as_too_large() {
    let (fake, links) = verified(LinkKind::Universal);
    let custom = format!("myapp://chat/{}", "a".repeat(MAX_URL_LEN));
    assert_eq!(
        code(delivered(&fake, &links, &custom).await),
        "ERR_DEEP_LINKS_URL_TOO_LARGE"
    );
    let web = format!("https://example.com/chat/{}", "a".repeat(MAX_URL_LEN));
    assert_eq!(
        code(delivered(&fake, &links, &web).await),
        "ERR_DEEP_LINKS_URL_TOO_LARGE"
    );
    // Exactly the limit is taken.
    let head = "myapp://chat/";
    let fits = format!("{head}{}", "a".repeat(MAX_URL_LEN - head.len()));
    assert_eq!(
        ok(delivered(&fake, &links, &fits).await).route.screen,
        "Chat"
    );
}

#[tokio::test(start_paused = true)]
async fn a_url_with_whitespace_or_control_characters_is_rejected() {
    let (fake, links) = verified(LinkKind::Universal);
    for url in [
        "myapp://chat/ja ne",
        "myapp://chat/jane\n",
        "myapp://chat/\tjane",
        "myapp://chat/jane\u{7f}",
        "myapp://chat/jane\u{a0}",
        "https://example.com/chat/ja ne",
        " myapp://chat/jane",
    ] {
        assert_eq!(
            code(delivered(&fake, &links, url).await),
            "ERR_DEEP_LINKS_INVALID_URL",
            "{url:?}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn a_link_of_an_unclaimed_scheme_is_rejected_as_disallowed() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    for url in [
        "other://chat/jane",
        "mailto:someone@example.com",
        "file:///etc/passwd",
        "javascript:alert(1)",
    ] {
        assert_eq!(
            code(delivered(&fake, &links, url).await),
            "ERR_DEEP_LINKS_DISALLOWED_SCHEME",
            "{url}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn an_https_link_to_an_unlisted_host_is_rejected_as_host_not_allowed() {
    let (fake, links) = verified(LinkKind::App);
    for url in [
        "https://evil.com/orders/42",
        "https://example.com.evil.com/orders/42",
        "https://sub.example.com/orders/42",
        "https://example.com:8443/orders/42",
        "http://example.com/orders/42",
    ] {
        assert_eq!(
            code(delivered(&fake, &links, url).await),
            "ERR_DEEP_LINKS_HOST_NOT_ALLOWED",
            "{url}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn scheme_and_host_match_case_insensitively() {
    let (fake, links) = verified(LinkKind::Universal);
    let custom = ok(delivered(&fake, &links, "MYAPP://chat/Jane").await);
    assert_eq!(custom.kind, LinkKind::Custom);
    assert_eq!(custom.route.params.get("author"), Some(&json!("Jane")));
    let web = ok(delivered(&fake, &links, "https://Example.COM/chat/JaNe").await);
    assert_eq!(web.kind, LinkKind::Universal);
    assert_eq!(web.route.params.get("author"), Some(&json!("JaNe")));
    // The path is not: `Chat` is `chat/...`.
    assert_eq!(
        code(delivered(&fake, &links, "myapp://CHAT/jane").await),
        "ERR_DEEP_LINKS_NO_ROUTE"
    );
}

#[tokio::test(start_paused = true)]
async fn a_verified_web_link_is_preferred_over_a_custom_scheme_link() {
    for kind in [LinkKind::Universal, LinkKind::App] {
        let (fake, links) = verified(kind);
        let link = ok(delivered(&fake, &links, "https://example.com/auth/callback?code=1").await);
        assert_eq!(link.kind, kind);
        assert_eq!(link.route.screen, "Login");
        assert!(link.url.starts_with("https://"));
        // The custom-scheme form of the same route is a different, lesser kind.
        let custom = ok(delivered(&fake, &links, "myapp://auth/callback?code=1").await);
        assert_eq!(custom.kind, LinkKind::Custom);
    }
}

#[tokio::test(start_paused = true)]
async fn a_verified_web_link_on_an_unsupported_platform_resolves_unsupported() {
    let links = DeepLinks::new(UnsupportedBackend, routes());
    assert_eq!(
        code(links.resolve("https://example.com/orders/42")),
        "ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM"
    );
    assert_eq!(
        code(bounded(links.next()).await),
        "ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM"
    );
    assert_eq!(ok(links.initial().await), None);
    // A platform with no verified links of its own answers the same for a delivered one.
    let fake = FakeBackend::new();
    let plain = DeepLinks::new(fake.clone(), routes());
    assert_eq!(
        code(delivered(&fake, &plain, "https://example.com/orders/42").await),
        "ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM"
    );
}

#[tokio::test(start_paused = true)]
async fn a_platform_failure_resolves_as_backend() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    let (answer, ()) = bounded(async {
        tokio::join!(links.next(), async {
            settle().await;
            fake.fail_platform();
        })
    })
    .await;
    assert_eq!(code(answer), "ERR_DEEP_LINKS_BACKEND");
    let broken = FakeBackend::new();
    broken.fail_initial(DeepLinksError::new(ErrorKind::Backend));
    assert_eq!(
        code(DeepLinks::new(broken, routes()).initial().await),
        "ERR_DEEP_LINKS_BACKEND"
    );
}

#[tokio::test(start_paused = true)]
async fn a_link_whose_path_matches_no_route_is_rejected_as_no_route() {
    let (fake, links) = verified(LinkKind::Universal);
    for url in [
        "myapp://invoices/42",
        "https://example.com/orders",
        "https://example.com/orders/42/lines",
    ] {
        assert_eq!(
            code(delivered(&fake, &links, url).await),
            "ERR_DEEP_LINKS_NO_ROUTE",
            "{url}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn query_params_and_fragments_are_parsed_and_preserved() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    let link = ok(delivered(
        &fake,
        &links,
        "myapp://orders/42?tab=lines&note=a%20b&flag#top",
    )
    .await);
    assert_eq!(link.url, "myapp://orders/42?tab=lines&note=a%20b&flag#top");
    let params = &link.route.params;
    assert_eq!(params.get("id"), Some(&json!(42)));
    assert_eq!(params.get("tab"), Some(&json!("lines")));
    assert_eq!(params.get("note"), Some(&json!("a b")));
    assert_eq!(params.get("flag"), Some(&json!("")));
    assert_eq!(params.get("top"), None);
    assert_eq!(params.len(), 4);
}

#[tokio::test(start_paused = true)]
async fn path_params_win_over_same_named_query_params() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    let link = ok(delivered(&fake, &links, "myapp://chat/jane?author=mallory&x=1").await);
    assert_eq!(link.route.params.get("author"), Some(&json!("jane")));
    assert_eq!(link.route.params.get("x"), Some(&json!("1")));
}

#[tokio::test(start_paused = true)]
async fn a_trailing_slash_matches_the_same_route() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    for url in [
        "myapp://orders/42",
        "myapp://orders/42/",
        "myapp://orders//42",
        "myapp:///orders/42//",
    ] {
        let link = ok(delivered(&fake, &links, url).await);
        assert_eq!(link.route.screen, "Order", "{url}");
        assert_eq!(link.route.params.get("id"), Some(&json!(42)), "{url}");
    }
}

#[tokio::test(start_paused = true)]
async fn an_empty_path_matches_the_root_route() {
    let (fake, links) = verified(LinkKind::App);
    for url in [
        "https://example.com/",
        "https://example.com",
        "myapp://",
        "https://example.com/?ref=mail",
    ] {
        assert_eq!(
            ok(delivered(&fake, &links, url).await).route.screen,
            "Home",
            "{url}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn percent_encoded_segments_decode_before_matching() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    let link = ok(delivered(&fake, &links, "myapp://chat/jane%20doe%2Fx").await);
    assert_eq!(link.route.params.get("author"), Some(&json!("jane doe/x")));
    // `%c3%a9` and `%C3%A9` are the same text.
    assert_eq!(
        ok(delivered(&fake, &links, "myapp://caf%c3%a9").await)
            .route
            .screen,
        "Cafe"
    );
    assert_eq!(
        ok(delivered(&fake, &links, "myapp://caf%C3%A9").await)
            .route
            .screen,
        "Cafe"
    );
    // A typed param decodes before its schema runs.
    let order = ok(delivered(&fake, &links, "myapp://orders/%34%32").await);
    assert_eq!(order.route.params.get("id"), Some(&json!(42)));
    for url in ["myapp://chat/%zz", "myapp://chat/%ff", "myapp://chat/%4"] {
        assert_eq!(
            code(delivered(&fake, &links, url).await),
            "ERR_DEEP_LINKS_NO_ROUTE",
            "{url}"
        );
    }
}

#[tokio::test(start_paused = true)]
async fn a_pending_link_survives_a_simulated_login() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    let order = ok(delivered(&fake, &links, "myapp://orders/42").await);
    // Not logged in: the app holds the link instead of routing it.
    links.pending().hold(order.clone());
    // The login completes through its own link, which routes while the slot keeps its link.
    let login = ok(delivered(&fake, &links, "myapp://auth/callback?code=xyz").await);
    assert_eq!(login.route.screen, "Login");
    assert_eq!(links.pending().held(), Some(order.clone()));
    assert_eq!(links.pending().claim(), Some(order));
}

#[tokio::test(start_paused = true)]
async fn claiming_a_pending_link_clears_the_slot() {
    let links = links(&FakeBackend::new());
    let pending = links.pending();
    let first = ok(links.resolve("myapp://orders/1"));
    let second = ok(links.resolve("myapp://orders/2"));
    pending.hold(first);
    // One slot: the newer link replaces the older one, without an error.
    pending.hold(second.clone());
    assert_eq!(pending.claim(), Some(second.clone()));
    assert_eq!(pending.claim(), None);
    pending.hold(second);
    pending.clear();
    assert_eq!(pending.held(), None);
    assert_eq!(pending.claim(), None);
}

#[tokio::test(start_paused = true)]
async fn links_waiting_past_max_pending_drop_the_oldest() {
    let fake = FakeBackend::new();
    let links = links(&fake);
    for i in 0..=MAX_PENDING {
        ok(fake.deliver(&format!("myapp://orders/{i}")));
    }
    for i in 1..=MAX_PENDING {
        let link = ok(bounded(links.next()).await);
        assert_eq!(
            link.route.params.get("id"),
            Some(&json!(i)),
            "in order, the oldest dropped"
        );
    }
    assert_eq!(
        code(links.next_within(Duration::from_secs(1)).await),
        "ERR_DEEP_LINKS_TIMEOUT"
    );
}

#[tokio::test(start_paused = true)]
async fn every_call_resolves_exactly_once() {
    let fake = FakeBackend::new();
    fake.set_initial(Some("myapp://orders/7"));
    let links = links(&fake);
    // initial: one platform read, one answer, repeated.
    let launch = ok(links.initial().await);
    assert_eq!(ok(links.initial().await), launch);
    assert_eq!(fake.initial_reads(), 1);
    // next: a link, then a coded failure; a failure scripted after the link resolved reaches
    // the next call, never the one that resolved.
    let link = ok(delivered(&fake, &links, "myapp://chat/jane").await);
    assert_eq!(link.route.screen, "Chat");
    fake.fail_platform();
    assert_eq!(code(bounded(links.next()).await), "ERR_DEEP_LINKS_BACKEND");
    assert_eq!(fake.deliveries(), 1);
    // A wait whose future is dropped frees the gate and cancels once.
    {
        let dropped = links.next();
        let _ = tokio::time::timeout(Duration::from_millis(1), dropped).await;
    }
    assert!(!fake.waiting());
    // claim: once.
    links.pending().hold(link.clone());
    assert_eq!(links.pending().claim(), Some(link));
    assert_eq!(links.pending().claim(), None);
    // stop: one cancel of the waiting call; every call after it answers at once.
    let (answer, stopped) = bounded(async {
        tokio::join!(links.next(), async {
            settle().await;
            links.stop().await
        })
    })
    .await;
    assert_eq!(code(answer), "ERR_DEEP_LINKS_CANCELLED");
    assert!(ok(stopped));
    assert!(!ok(links.stop().await));
    assert_eq!(
        code(bounded(links.next()).await),
        "ERR_DEEP_LINKS_CANCELLED"
    );
}
