//! `DeepLinks`: the calls an app makes — the link that started it, the links that arrive while
//! it runs, and the route a link maps to. One wait runs at a time, and every call resolves
//! exactly once.

use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use nsh_core::url_scheme::{check_url, MAX_URL_LEN};
use url::Url;

use crate::{
    backend::Backend,
    error::{DeepLinksError, ErrorKind},
    routes::{RouteMatch, RouteTable},
};

/// How a link reached the app. Verified web links are one kind per platform; custom schemes
/// are the app's own, claimed in its prefixes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LinkKind {
    /// A Universal Link: an `http(s)` link the web vouches for on iOS and macOS.
    Universal,
    /// An App Link: an `http(s)` link the web vouches for on Android.
    App,
    /// A URL of a scheme the app claims (`myapp://`).
    Custom,
}

impl LinkKind {
    /// The name TSX and the native bridge use: `"universal"`, `"app"`, `"custom"`.
    pub fn as_str(self) -> &'static str {
        match self {
            LinkKind::Universal => "universal",
            LinkKind::App => "app",
            LinkKind::Custom => "custom",
        }
    }

    /// The kind a name names; `None` for any other.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "universal" => Some(LinkKind::Universal),
            "app" => Some(LinkKind::App),
            "custom" => Some(LinkKind::Custom),
            _ => None,
        }
    }
}

/// One link the system handed the app, checked and classified.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Link {
    /// The URL as it came in.
    pub url: String,
    /// How it reached the app.
    pub kind: LinkKind,
}

impl Link {
    /// The route this link maps to; `ERR_DEEP_LINKS_NO_ROUTE` when its path matches none.
    pub fn route(&self, routes: &RouteTable) -> Result<RouteMatch, DeepLinksError> {
        routes.match_url(&self.url)
    }
}

struct Shared {
    backend: Arc<dyn Backend>,
    routes: Mutex<RouteTable>,
    /// Whether one `next` waits. One wait runs at a time; a second gets `Busy`.
    busy: Mutex<bool>,
    initial: Mutex<Option<Result<Option<Link>, DeepLinksError>>>,
    held: Mutex<Option<Link>>,
}

impl Shared {
    fn busy(&self) -> MutexGuard<'_, bool> {
        self.busy.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn routes(&self) -> MutexGuard<'_, RouteTable> {
        self.routes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Deep links for one app, as `app_links` names its calls: the initial link (cold start), the
/// next one to arrive, and a stop that cancels the wait. Clones share one state, so two calls
/// in one app share the wait too.
#[derive(Clone)]
pub struct DeepLinks {
    shared: Arc<Shared>,
}

impl DeepLinks {
    /// Deep links through `backend`, routed by `routes`. Tests and fakes use this.
    pub fn new(backend: impl Backend + 'static, routes: RouteTable) -> Self {
        Self::from_backend(Arc::new(backend), routes)
    }

    /// Deep links through the platform this build runs on.
    pub fn platform(routes: RouteTable) -> Self {
        Self::from_backend(crate::platform::default_backend(), routes)
    }

    fn from_backend(backend: Arc<dyn Backend>, routes: RouteTable) -> Self {
        Self {
            shared: Arc::new(Shared {
                backend,
                routes: Mutex::new(routes),
                busy: Mutex::new(false),
                initial: Mutex::new(None),
                held: Mutex::new(None),
            }),
        }
    }

    /// Replaces the route table. The next link routes through the new one.
    pub fn set_routes(&self, routes: RouteTable) {
        *self.shared.routes() = routes;
    }

    /// The route table as it stands.
    pub fn routes(&self) -> RouteTable {
        self.shared.routes().clone()
    }

    /// The link that started the app, cold-start semantics: the platform reports it once and
    /// the answer stands for every later call. `Ok(None)` when the app started without one.
    pub async fn initial(&self) -> Result<Option<Link>, DeepLinksError> {
        let cached = {
            let initial = self
                .shared
                .initial
                .lock()
                .unwrap_or_else(PoisonError::into_inner);
            initial.clone()
        };
        if let Some(answer) = cached {
            return answer;
        }
        let raw = self.shared.backend.initial_url().await?;
        let answer = match raw {
            None => Ok(None),
            Some(raw) => classify(&self.shared, &raw).map(Some),
        };
        *self
            .shared
            .initial
            .lock()
            .unwrap_or_else(PoisonError::into_inner) = Some(answer.clone());
        answer
    }

    /// Waits for the next link the system hands the app, checks it, and classifies it. A
    /// second call while one waits gets `ERR_DEEP_LINKS_BUSY`; `stop` cancels it with
    /// `ERR_DEEP_LINKS_CANCELLED`; a delivered link that fails the checks is the coded
    /// failure itself (`ERR_DEEP_LINKS_DISALLOWED_SCHEME`, and so on). Dropping the future
    /// cancels the wait.
    pub async fn next(&self) -> Result<Link, DeepLinksError> {
        {
            let mut busy = self.shared.busy();
            if *busy {
                return Err(DeepLinksError::new(ErrorKind::Busy));
            }
            *busy = true;
        }
        let owner = Owner {
            shared: Arc::clone(&self.shared),
            armed: true,
        };
        let raw = self.shared.backend.next_url().await;
        *self.shared.busy() = false;
        owner.armed = false;
        classify(&self.shared, &raw?)
    }

    /// Cancels the pending `next`, which resolves with `ERR_DEEP_LINKS_CANCELLED`. Returns
    /// `true` when a wait was pending.
    pub async fn stop(&self) -> Result<bool, DeepLinksError> {
        let busy = *self.shared.busy();
        if busy {
            self.shared.backend.cancel_wait();
        }
        Ok(busy)
    }

    /// The pending-link slot: one link held while the app is not ready to route it, claimed
    /// when it is. What it holds survives anything but `clear`, a login included.
    pub fn pending(&self) -> PendingLink {
        PendingLink {
            shared: Arc::clone(&self.shared),
        }
    }

    /// The route a URL maps to, without waiting for anything. The same checks and codes as
    /// [`RouteTable::match_url`].
    pub fn route(&self, url: &str) -> Result<RouteMatch, DeepLinksError> {
        if url.len() > MAX_URL_LEN {
            return Err(DeepLinksError::new(ErrorKind::UrlTooLarge));
        }
        self.shared.routes().match_url(url)
    }
}

/// One held link, held while the app cannot route it (before a login, say) and claimed after.
/// Clones share the slot with the `DeepLinks` they came from.
#[derive(Clone)]
pub struct PendingLink {
    shared: Arc<Shared>,
}

impl PendingLink {
    /// Holds `link`. One slot: a second link replaces the first, as NSH core's pending links
    /// keep the newest.
    pub fn hold(&self, link: Link) {
        *self.held_slot() = Some(link);
    }

    /// Takes the held link, `None` when the slot is empty.
    pub fn claim(&self) -> Option<Link> {
        self.held_slot().take()
    }

    /// Empties the slot.
    pub fn clear(&self) {
        *self.held_slot() = None;
    }

    /// The held link, without taking it.
    pub fn held(&self) -> Option<Link> {
        self.held_slot().clone()
    }

    fn held_slot(&self) -> MutexGuard<'_, Option<Link>> {
        self.shared
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Cancels the wait if the call's future is dropped before the answer arrives.
struct Owner {
    shared: Arc<Shared>,
    armed: bool,
}

impl Drop for Owner {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        *self.shared.busy() = false;
        self.shared.backend.cancel_wait();
    }
}

/// The pipeline every link goes through: NSH core's check against the claimed schemes, then
/// the verified-web-link layer this crate adds — the host allowlist of the route table's
/// `http(s)` prefixes, and the platform's word for what a verified link is.
fn classify(shared: &Shared, raw: &str) -> Result<Link, DeepLinksError> {
    if raw.len() > MAX_URL_LEN {
        return Err(DeepLinksError::new(ErrorKind::UrlTooLarge));
    }
    let routes = shared.routes();
    let claimed = routes.claimed_schemes();
    if check_url(raw, &claimed).is_ok() {
        return Ok(Link {
            url: raw.to_owned(),
            kind: LinkKind::Custom,
        });
    }
    let parsed = Url::parse(raw).map_err(|_| DeepLinksError::new(ErrorKind::InvalidUrl))?;
    if !matches!(parsed.scheme(), "http" | "https") {
        return Err(DeepLinksError::new(ErrorKind::DisallowedScheme));
    }
    let Some(host) = parsed.host_str() else {
        return Err(DeepLinksError::new(ErrorKind::InvalidUrl));
    };
    if !routes.allows_host(host) {
        return Err(DeepLinksError::new(ErrorKind::HostNotAllowed));
    }
    match shared.backend.web_link_kind() {
        Some(kind) => Ok(Link {
            url: raw.to_owned(),
            kind,
        }),
        None => Err(DeepLinksError::new(ErrorKind::UnsupportedPlatform)),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{backend::Backend, fake::FakeBackend, links::LinkKind};
    use serde_json::json;

    fn routes() -> RouteTable {
        RouteTable::from_config(&json!({
            "prefixes": ["https://example.com", "myapp://"],
            "screens": { "Order": "orders/:id" }
        }))
        .ok()
        .expect("a valid config")
    }

    fn links(fake: &FakeBackend) -> DeepLinks {
        DeepLinks::new(fake.clone(), routes())
    }

    async fn settle(ready: impl Fn() -> bool) {
        for _ in 0..1000 {
            if ready() {
                return;
            }
            tokio::task::yield_now().await;
        }
        panic!("the wait never started");
    }

    #[test]
    fn a_kind_has_the_name_tsx_uses() {
        assert_eq!(LinkKind::Universal.as_str(), "universal");
        assert_eq!(LinkKind::App.as_str(), "app");
        assert_eq!(LinkKind::Custom.as_str(), "custom");
        assert_eq!(LinkKind::from_name("app"), Some(LinkKind::App));
        assert_eq!(LinkKind::from_name("web"), None);
    }

    #[tokio::test]
    async fn a_link_of_a_claimed_scheme_is_custom_and_routes() {
        let fake = FakeBackend::new();
        let links = links(&fake);
        let link = links.next().await.ok().expect("a custom link");
        assert_eq!(link.kind, LinkKind::Custom);
        let routed = link.route(&links.routes()).ok().expect("a route");
        assert_eq!(routed.screen, "Order");
    }

    #[tokio::test]
    async fn every_link_the_checks_refuse_is_a_coded_failure() {
        for (delivered, code) in [
            ("https://evil.com/orders/42", "ERR_DEEP_LINKS_HOST_NOT_ALLOWED"),
            ("other://open", "ERR_DEEP_LINKS_DISALLOWED_SCHEME"),
            ("not a url", "ERR_DEEP_LINKS_INVALID_URL"),
            ("/relative", "ERR_DEEP_LINKS_INVALID_URL"),
            ("https://example.com/a b", "ERR_DEEP_LINKS_INVALID_URL"),
            ("myapp://open\n", "ERR_DEEP_LINKS_INVALID_URL"),
        ] {
            let fake = FakeBackend::new();
            let links = links(&fake);
            let wait = links.next();
            fake.fail_next_delivery(delivered.to_owned());
            let answer = wait.await.err().map(|e| e.code().to_owned());
            assert_eq!(answer.as_deref(), Some(code), "{delivered}");
        }
    }

    #[tokio::test]
    async fn a_link_over_the_limit_is_refused_by_its_size() {
        let fake = FakeBackend::new();
        let links = links(&fake);
        let wait = links.next();
        let huge = format!("myapp://{}", "a".repeat(MAX_URL_LEN));
        fake.fail_next_delivery(huge);
        assert_eq!(
            wait.await.err().code(),
            "ERR_DEEP_LINKS_URL_TOO_LARGE"
        );
    }

    #[tokio::test]
    async fn a_verified_web_link_is_the_platforms_kind_or_unsupported() {
        let fake = FakeBackend::new();
        let links = links(&fake);
        let wait = links.next();
        fake.fail_next_delivery("https://example.com/orders/42".to_owned());
        assert_eq!(
            wait.await.err().code(),
            "ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM"
        );

        for kind in [LinkKind::Universal, LinkKind::App] {
            let fake = FakeBackend::new();
            fake.set_web_link_kind(Some(kind));
            let links = links(&fake);
            let wait = links.next();
            fake.fail_next_delivery("https://example.com/orders/42".to_owned());
            assert_eq!(wait.await.ok().map(|l| l.kind), Some(kind));
        }
    }

    #[tokio::test]
    async fn the_initial_link_is_answered_once_and_stands() {
        let fake = FakeBackend::new();
        fake.set_initial(Some("myapp://orders/42".to_owned()));
        let links = links(&fake);
        let first = links.initial().await.ok().flatten();
        assert_eq!(
            first.map(|l| (l.kind, l.url)),
            Some((LinkKind::Custom, "myapp://orders/42".to_owned()))
        );
        let second = links.initial().await.ok().flatten();
        assert_eq!(second.map(|l| l.url), Some("myapp://orders/42".to_owned()));
        assert_eq!(fake.initial_reads(), 1);

        let none = links(&FakeBackend::new());
        assert_eq!(none.initial().await.ok(), Some(None));
    }

    #[tokio::test]
    async fn a_queued_link_reaches_the_next_wait_in_order() {
        let fake = FakeBackend::new();
        let links = links(&fake);
        fake.deliver("myapp://orders/1");
        fake.deliver("myapp://orders/2");
        assert_eq!(
            links.next().await.ok().map(|l| l.url),
            Some("myapp://orders/1".to_owned())
        );
        assert_eq!(
            links.next().await.ok().map(|l| l.url),
            Some("myapp://orders/2".to_owned())
        );
    }

    #[tokio::test]
    async fn one_wait_runs_at_a_time() {
        let fake = FakeBackend::new();
        let links = links(&fake);
        let waiting = tokio::spawn({
            let links = links.clone();
            async move { links.next().await }
        });
        settle(|| fake.waiting()).await;
        let second = links.next().await.err();
        assert_eq!(second.map(DeepLinksError::code), Some("ERR_DEEP_LINKS_BUSY"));
        fake.cancel_wait();
        let cancelled = waiting.await.ok().flatten().err();
        assert_eq!(
            cancelled.map(DeepLinksError::code),
            Some("ERR_DEEP_LINKS_CANCELLED")
        );
    }

    #[tokio::test]
    async fn stop_cancels_the_wait_and_reports_it() {
        let fake = FakeBackend::new();
        let links = links(&fake);
        let waiting = tokio::spawn({
            let links = links.clone();
            async move { links.next().await }
        });
        settle(|| fake.waiting()).await;
        assert_eq!(links.stop().await.ok(), Some(true));
        let answer = waiting.await.ok().flatten().err();
        assert_eq!(
            answer.map(DeepLinksError::code),
            Some("ERR_DEEP_LINKS_CANCELLED")
        );
        assert_eq!(links.stop().await.ok(), Some(false));
    }

    #[tokio::test]
    async fn a_failed_wait_resolves_with_its_code() {
        let fake = FakeBackend::new();
        let links = links(&fake);
        let wait = links.next();
        fake.fail_next(DeepLinksError::new(ErrorKind::Timeout));
        assert_eq!(wait.await.err().code(), "ERR_DEEP_LINKS_TIMEOUT");
        // The gate is free again.
        fake.deliver("myapp://orders/42");
        assert!(links.next().await.is_ok());
    }

    #[tokio::test]
    async fn a_dropped_wait_frees_the_gate() {
        let fake = FakeBackend::new();
        let links = links(&fake);
        let waiting = tokio::spawn({
            let links = links.clone();
            async move { links.next().await }
        });
        settle(|| fake.waiting()).await;
        waiting.abort();
        settle(|| !fake.waiting()).await;
        fake.deliver("myapp://orders/42");
        assert!(links.next().await.is_ok());
    }

    #[tokio::test]
    async fn the_pending_slot_holds_claims_and_clears() {
        let fake = FakeBackend::new();
        let links = links(&fake);
        let pending = links.pending();
        assert_eq!(pending.held(), None);
        let link = Link {
            url: "myapp://orders/42".to_owned(),
            kind: LinkKind::Custom,
        };
        pending.hold(link.clone());
        // A login changes nothing the slot holds.
        assert_eq!(pending.held(), Some(link.clone()));
        let second = Link {
            url: "myapp://orders/43".to_owned(),
            kind: LinkKind::Custom,
        };
        pending.hold(second.clone());
        assert_eq!(pending.claim(), Some(second));
        assert_eq!(pending.claim(), None);
        pending.hold(link);
        pending.clear();
        assert_eq!(pending.held(), None);
    }

    #[tokio::test]
    async fn a_url_routes_without_waiting() {
        let fake = FakeBackend::new();
        let links = links(&fake);
        let routed = links.route("myapp://orders/42").ok().expect("a route");
        assert_eq!(routed.screen, "Order");
        assert_eq!(
            links.route("myapp://invoices/42").err().code(),
            "ERR_DEEP_LINKS_NO_ROUTE"
        );
    }
}
