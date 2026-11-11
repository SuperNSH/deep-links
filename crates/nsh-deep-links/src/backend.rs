//! The seam between [`crate::DeepLinks`] and a platform: the raw URLs the system hands the
//! app. Every call resolves exactly once; validation, classification and routing stay on the
//! crate's side of the seam.

use std::{future::Future, pin::Pin};

use crate::{error::DeepLinksError, links::LinkKind};

/// A future a backend returns. Each call's future borrows the backend.
pub type BackendFuture<'a, T> = Pin<Box<dyn Future<Output = T> + 'a>>;

/// What a platform provides: the link the app was started with, and the links the system hands
/// it while it runs. [`crate::DeepLinks`] adds the rules: one wait at a time, one answer per
/// call, every link checked and routed before the app sees it.
///
/// Each `next_url` future resolves exactly once: `Ok(url)` when the system hands one over,
/// `Err` with a code when the platform fails, the deadline passes, or the wait is cancelled.
/// `cancel_wait` resolves a pending `next_url` with `ERR_DEEP_LINKS_CANCELLED`; a platform
/// that cannot deliver links at all resolves it at once with
/// `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM`, never leaving it hanging. Links that arrive with no
/// wait pending are kept, at most [`crate::MAX_PENDING`], the oldest dropped.
pub trait Backend: Send + Sync {
    /// The URL the app was started with (cold start), if the system gives one. Reported
    /// unchanged: whether it is the app's to take is decided above the seam.
    fn initial_url(&self) -> BackendFuture<'_, Result<Option<String>, DeepLinksError>>;

    /// The next URL the system hands the app. Resolves exactly once.
    fn next_url(&self) -> BackendFuture<'_, Result<String, DeepLinksError>>;

    /// Cancels the pending `next_url`, if one waits. Where none waits, this does nothing.
    fn cancel_wait(&self);

    /// The kind the platform's verified `http(s)` links are: Universal Links on Apple, App
    /// Links on Android. `None` where the platform has none: there a verified web link is a
    /// coded `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM`.
    fn web_link_kind(&self) -> Option<LinkKind>;
}
