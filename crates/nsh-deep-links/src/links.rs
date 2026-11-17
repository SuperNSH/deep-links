//! `DeepLinks`: the link that started the app, the links that arrive while it runs, and the
//! pending-link slot. One wait runs at a time, and every call resolves exactly once.

use std::{
    sync::{Arc, Mutex, MutexGuard, PoisonError},
    time::Duration,
};

use nsh_core::url_scheme::{MAX_URL_LEN, check_url};
use tokio::sync::OnceCell;
use url::Url;

use crate::{
    backend::Backend,
    error::{DeepLinksError, ErrorKind},
    routes::{RouteMatch, RouteTable},
};

/// How a link reached the app. The kind comes from how the system delivered it and is never
/// downgraded to make a link match.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[non_exhaustive]
pub enum LinkKind {
    /// A Universal Link: an `https` link the web vouches for, on iOS and macOS.
    Universal,
    /// An App Link: an `https` link the web vouches for, on Android.
    App,
    /// A URL of a scheme the app claims (`myapp://`), through NSH core's `UrlScheme`.
    Custom,
    /// The web: the page's own address when the app loaded.
    Web,
}

impl LinkKind {
    /// The name TSX and the native bridge use: `"universal"`, `"app"`, `"custom"`, `"web"`.
    pub fn as_str(self) -> &'static str {
        match self {
            LinkKind::Universal => "universal",
            LinkKind::App => "app",
            LinkKind::Custom => "custom",
            LinkKind::Web => "web",
        }
    }

    /// The kind a name names; `None` for any other.
    pub fn from_name(name: &str) -> Option<Self> {
        match name {
            "universal" => Some(LinkKind::Universal),
            "app" => Some(LinkKind::App),
            "custom" => Some(LinkKind::Custom),
            "web" => Some(LinkKind::Web),
            _ => None,
        }
    }
}

/// One link the system handed the app: checked, classified and routed.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    /// The URL as it came in, fragment included.
    pub url: String,
    /// How it reached the app.
    pub kind: LinkKind,
    /// The screen it routes to.
    pub route: RouteMatch,
}

#[derive(Default)]
struct Gate {
    busy: bool,
    stopped: bool,
}

struct Shared {
    backend: Arc<dyn Backend>,
    routes: Mutex<RouteTable>,
    gate: Mutex<Gate>,
    initial: OnceCell<Result<Option<Link>, DeepLinksError>>,
    held: Mutex<Option<Link>>,
}

impl Shared {
    fn gate(&self) -> MutexGuard<'_, Gate> {
        self.gate.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn routes(&self) -> MutexGuard<'_, RouteTable> {
        self.routes.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

/// Deep links for one app, after `app_links`: [`initial`](Self::initial) is the link that
/// started it, [`next`](Self::next) the next one to arrive, [`stop`](Self::stop) ends delivery.
/// Clones share one state, so they share the one wait too.
#[derive(Clone)]
pub struct DeepLinks {
    shared: Arc<Shared>,
}

impl DeepLinks {
    /// Deep links through `backend`, routed by `routes`. Tests and fakes use this.
    pub fn new(backend: impl Backend + 'static, routes: RouteTable) -> Self {
        Self::with_backend(Arc::new(backend), routes)
    }

    /// Deep links through the platform this build runs on.
    pub fn platform(routes: RouteTable) -> Self {
        Self::with_backend(crate::platform::default_backend(), routes)
    }

    /// Deep links through a shared backend.
    pub fn with_backend(backend: Arc<dyn Backend>, routes: RouteTable) -> Self {
        Self {
            shared: Arc::new(Shared {
                backend,
                routes: Mutex::new(routes),
                gate: Mutex::new(Gate::default()),
                initial: OnceCell::new(),
                held: Mutex::new(None),
            }),
        }
    }

    /// The route table links route through.
    pub fn routes(&self) -> RouteTable {
        self.shared.routes().clone()
    }

    /// The link that started the app (`app_links`' `getInitialLink`): read from the platform
    /// once, and the same answer on every later call. `Ok(None)` when no link started it. A
    /// launch link that fails the checks is that failure, every time.
    pub async fn initial(&self) -> Result<Option<Link>, DeepLinksError> {
        let shared = &self.shared;
        shared
            .initial
            .get_or_init(|| async {
                match shared.backend.initial_url().await? {
                    None => Ok(None),
                    Some(raw) => resolve(shared, &raw).map(Some),
                }
            })
            .await
            .clone()
    }

    /// Waits for the next link the system hands the app, then checks, classifies and routes
    /// it. A link that fails is the coded failure itself. A second call while one waits is
    /// `ERR_DEEP_LINKS_BUSY`; [`stop`](Self::stop) resolves the wait with
    /// `ERR_DEEP_LINKS_CANCELLED`. Dropping the future cancels the wait.
    pub async fn next(&self) -> Result<Link, DeepLinksError> {
        self.wait(None).await
    }

    /// [`next`](Self::next), giving up with `ERR_DEEP_LINKS_TIMEOUT` once `deadline` passes with
    /// no link. On the web, which has no timer driver here, the deadline is not armed: its wait
    /// resolves at once anyway.
    pub async fn next_within(&self, deadline: Duration) -> Result<Link, DeepLinksError> {
        if deadline.is_zero() {
            return Err(DeepLinksError::new(ErrorKind::InvalidArgument)
                .with_detail("a deadline is longer than zero"));
        }
        self.wait(Some(deadline)).await
    }

    async fn wait(&self, deadline: Option<Duration>) -> Result<Link, DeepLinksError> {
        {
            let mut gate = self.shared.gate();
            if gate.stopped {
                return Err(
                    DeepLinksError::new(ErrorKind::Cancelled).with_detail("delivery was stopped")
                );
            }
            if gate.busy {
                return Err(DeepLinksError::new(ErrorKind::Busy));
            }
            gate.busy = true;
        }
        let mut owner = Owner {
            shared: Arc::clone(&self.shared),
            armed: true,
        };
        let raw = match deadline {
            #[cfg(not(target_arch = "wasm32"))]
            Some(deadline) => {
                tokio::select! {
                    raw = self.shared.backend.next_url() => raw,
                    _ = tokio::time::sleep(deadline) => {
                        self.shared.backend.cancel_wait();
                        Err(DeepLinksError::new(ErrorKind::Timeout))
                    }
                }
            }
            _ => self.shared.backend.next_url().await,
        };
        owner.armed = false;
        self.shared.gate().busy = false;
        resolve(&self.shared, &raw?)
    }

    /// Ends delivery: the waiting `next` resolves with `ERR_DEEP_LINKS_CANCELLED`, and so does
    /// every later one; links that arrive afterwards are never handed out. Build a new
    /// `DeepLinks` to listen again. Returns `true` when a wait was cancelled.
    pub async fn stop(&self) -> Result<bool, DeepLinksError> {
        let busy = {
            let mut gate = self.shared.gate();
            gate.stopped = true;
            gate.busy
        };
        if busy {
            self.shared.backend.cancel_wait();
        }
        Ok(busy)
    }

    /// The pending-link slot: one link held while the app cannot route it yet.
    pub fn pending(&self) -> PendingLink {
        PendingLink {
            shared: Arc::clone(&self.shared),
        }
    }

    /// Runs `url` through the same checks, classification and routing as a delivered link,
    /// without waiting for anything: for an address the app already has.
    pub fn resolve(&self, url: &str) -> Result<Link, DeepLinksError> {
        resolve(&self.shared, url)
    }
}

/// One held link, held while the app cannot route it (before a login, say) and claimed after.
/// Clones share the slot with the `DeepLinks` they came from.
#[derive(Clone)]
pub struct PendingLink {
    shared: Arc<Shared>,
}

impl PendingLink {
    /// Holds `link`. The slot holds one: a newer link replaces the held one, the oldest going,
    /// as NSH core's pending links do.
    pub fn hold(&self, link: Link) {
        *self.slot() = Some(link);
    }

    /// Takes the held link: it is handed out once, and the slot is empty after.
    pub fn claim(&self) -> Option<Link> {
        self.slot().take()
    }

    /// Empties the slot.
    pub fn clear(&self) {
        *self.slot() = None;
    }

    /// The held link, left in the slot.
    pub fn held(&self) -> Option<Link> {
        self.slot().clone()
    }

    fn slot(&self) -> MutexGuard<'_, Option<Link>> {
        self.shared
            .held
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }
}

/// Frees the gate and cancels the platform's wait when a `next` future is dropped mid-wait.
struct Owner {
    shared: Arc<Shared>,
    armed: bool,
}

impl Drop for Owner {
    fn drop(&mut self) {
        if self.armed {
            self.shared.gate().busy = false;
            self.shared.backend.cancel_wait();
        }
    }
}

/// The pipeline every link goes through. NSH core's `check_url` decides the custom schemes and
/// applies the same length and character rules to verified `http(s)` links; then the host
/// allowlist, the platform's kind for verified links, and the route.
fn resolve(shared: &Shared, raw: &str) -> Result<Link, DeepLinksError> {
    let routes = shared.routes();
    let web = ["http".to_owned(), "https".to_owned()];
    let kind = if check_url(raw, &web).is_ok() {
        let parsed = Url::parse(raw).map_err(|_| DeepLinksError::new(ErrorKind::InvalidUrl))?;
        let host = parsed
            .host_str()
            .ok_or_else(|| DeepLinksError::new(ErrorKind::InvalidUrl))?;
        let authority = match parsed.port() {
            Some(port) => format!("{host}:{port}"),
            None => host.to_owned(),
        };
        if !routes.allows_host(parsed.scheme(), &authority) {
            return Err(DeepLinksError::new(ErrorKind::HostNotAllowed));
        }
        shared.backend.web_link_kind().ok_or_else(|| {
            DeepLinksError::new(ErrorKind::UnsupportedPlatform)
                .with_detail("this platform delivers no verified web links")
        })?
    } else if check_url(raw, &routes.claimed_schemes()).is_ok() {
        LinkKind::Custom
    } else {
        return Err(refusal(raw));
    };
    let route = routes.match_url(raw)?;
    Ok(Link {
        url: raw.to_owned(),
        kind,
        route,
    })
}

/// The code for a URL core's `check_url` refused for every scheme the app takes.
fn refusal(raw: &str) -> DeepLinksError {
    if raw.len() > MAX_URL_LEN {
        DeepLinksError::new(ErrorKind::UrlTooLarge)
    } else if raw.chars().any(|c| c.is_whitespace() || c.is_control()) {
        DeepLinksError::new(ErrorKind::InvalidUrl)
            .with_detail("a link has no whitespace or control characters")
    } else if Url::parse(raw).is_err() {
        DeepLinksError::new(ErrorKind::InvalidUrl).with_detail("a link is an absolute URL")
    } else {
        DeepLinksError::new(ErrorKind::DisallowedScheme)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_kind_has_the_name_tsx_uses() {
        for kind in [
            LinkKind::Universal,
            LinkKind::App,
            LinkKind::Custom,
            LinkKind::Web,
        ] {
            assert_eq!(LinkKind::from_name(kind.as_str()), Some(kind));
        }
        assert_eq!(LinkKind::from_name("Universal"), None);
    }

    #[test]
    fn a_refused_url_is_named_by_why() {
        let code = |raw: &str| refusal(raw).code();
        assert_eq!(
            code(&"a".repeat(MAX_URL_LEN + 1)),
            "ERR_DEEP_LINKS_URL_TOO_LARGE"
        );
        assert_eq!(code("myapp://a b"), "ERR_DEEP_LINKS_INVALID_URL");
        assert_eq!(code("/relative"), "ERR_DEEP_LINKS_INVALID_URL");
        assert_eq!(code(""), "ERR_DEEP_LINKS_INVALID_URL");
        assert_eq!(code("other://open"), "ERR_DEEP_LINKS_DISALLOWED_SCHEME");
    }
}
