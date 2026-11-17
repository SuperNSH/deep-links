//! The delivery rules every backend shares: one wait at a time, links that arrive with no wait
//! held in order up to [`MAX_PENDING`] (the oldest going), and a duplicate of the link a wait is
//! resolving with refused as `ERR_DEEP_LINKS_BUSY` rather than delivered twice.

use std::{
    collections::VecDeque,
    sync::{Mutex, MutexGuard, PoisonError},
};

use tokio::sync::oneshot;

use crate::{
    MAX_PENDING,
    backend::BackendFuture,
    error::{DeepLinksError, ErrorKind},
};

type Waiter = oneshot::Sender<Result<String, DeepLinksError>>;

#[derive(Default)]
struct State {
    queued: VecDeque<String>,
    waiting: Option<Waiter>,
    /// The URL the last wait resolved with, until the next wait starts.
    resolving: Option<String>,
    next_error: Option<DeepLinksError>,
    deliveries: usize,
    cancels: usize,
}

/// Where a platform's callbacks drop the URLs the system hands the app, and where `next_url`
/// waits for them.
#[derive(Default)]
pub(crate) struct Inbox {
    state: Mutex<State>,
}

impl Inbox {
    pub(crate) const fn new() -> Self {
        Self {
            state: Mutex::new(State {
                queued: VecDeque::new(),
                waiting: None,
                resolving: None,
                next_error: None,
                deliveries: 0,
                cancels: 0,
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Hands `url` to the waiting call, else holds it. A copy of the URL the last wait resolved
    /// with, arriving before the next wait starts, is the system firing twice: `Busy`.
    pub(crate) fn deliver(&self, url: &str) -> Result<(), DeepLinksError> {
        let mut state = self.lock();
        // A wait whose caller went away (a deadline, a dropped future) takes nothing.
        if let Some(waiting) = state.waiting.take().filter(|w| !w.is_closed()) {
            state.deliveries += 1;
            state.resolving = Some(url.to_owned());
            let _ = waiting.send(Ok(url.to_owned()));
            return Ok(());
        }
        if state.resolving.as_deref() == Some(url) {
            return Err(DeepLinksError::new(ErrorKind::Busy)
                .with_detail("the same link arrived again while the last wait resolved with it"));
        }
        if state.queued.len() >= MAX_PENDING {
            state.queued.pop_front();
        }
        state.queued.push_back(url.to_owned());
        Ok(())
    }

    /// Fails the waiting call with `error`, else the next one, before any held link.
    pub(crate) fn fail(&self, error: DeepLinksError) {
        let mut state = self.lock();
        if let Some(waiting) = state.waiting.take().filter(|w| !w.is_closed()) {
            let _ = waiting.send(Err(error));
            return;
        }
        state.next_error = Some(error);
    }

    /// The next URL: a scripted failure first, then the oldest held link, else a wait.
    pub(crate) fn next_url(&self) -> BackendFuture<'static, Result<String, DeepLinksError>> {
        let mut state = self.lock();
        state.resolving = None;
        if let Some(error) = state.next_error.take() {
            return Box::pin(std::future::ready(Err(error)));
        }
        if let Some(url) = state.queued.pop_front() {
            state.deliveries += 1;
            state.resolving = Some(url.clone());
            return Box::pin(std::future::ready(Ok(url)));
        }
        let (sender, receiver) = oneshot::channel();
        // A newer wait replaces an abandoned one; the abandoned receiver is gone already.
        state.waiting = Some(sender);
        Box::pin(async move {
            receiver
                .await
                .unwrap_or_else(|_| Err(DeepLinksError::new(ErrorKind::Cancelled)))
        })
    }

    /// Resolves the waiting call with `ERR_DEEP_LINKS_CANCELLED`; nothing when none waits.
    pub(crate) fn cancel(&self) {
        let mut state = self.lock();
        state.cancels += 1;
        if let Some(waiting) = state.waiting.take() {
            let _ = waiting.send(Err(DeepLinksError::new(ErrorKind::Cancelled)));
        }
    }

    pub(crate) fn waiting(&self) -> bool {
        self.lock()
            .waiting
            .as_ref()
            .is_some_and(|waiting| !waiting.is_closed())
    }

    pub(crate) fn queued(&self) -> Vec<String> {
        self.lock().queued.iter().cloned().collect()
    }

    pub(crate) fn deliveries(&self) -> usize {
        self.lock().deliveries
    }

    pub(crate) fn cancels(&self) -> usize {
        self.lock().cancels
    }
}
