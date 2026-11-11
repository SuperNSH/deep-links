//! A backend whose deliveries the test scripts, and whose waits it can cancel or fail. Tests
//! use it, and so can an app that runs without a platform behind it.

use std::{
    collections::VecDeque,
    future::ready,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use tokio::sync::oneshot;

use crate::{
    backend::{Backend, BackendFuture},
    error::{DeepLinksError, ErrorKind},
    links::LinkKind,
    MAX_PENDING,
};

/// A backend with a scripted initial link, a queue of deliveries, and a next answer the test
/// can fail. Clones share one state.
///
/// Like the platforms, a cancelled wait resolves with `ERR_DEEP_LINKS_CANCELLED`, and a
/// delivery with no wait pending waits in the queue, the oldest dropped past
/// [`MAX_PENDING`].
#[derive(Clone)]
pub struct FakeBackend {
    state: Arc<Mutex<FakeState>>,
}

#[derive(Default)]
struct FakeState {
    initial: Option<String>,
    queued: VecDeque<String>,
    next_error: Option<DeepLinksError>,
    waiting: Option<oneshot::Sender<Result<String, DeepLinksError>>>,
    web_kind: Option<LinkKind>,
    deliveries: usize,
    cancels: usize,
    initial_reads: usize,
}

impl Default for FakeBackend {
    /// A backend with nothing scripted: no initial link, no deliveries.
    fn default() -> Self {
        Self::new()
    }
}

impl FakeBackend {
    /// A backend with nothing scripted.
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(FakeState::default())),
        }
    }

    fn lock(&self) -> MutexGuard<'_, FakeState> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets the link the app was started with.
    pub fn set_initial(&self, url: Option<String>) {
        self.lock().initial = url;
    }

    /// Hands a URL over: to a waiting call at once, else to the queue, its oldest link
    /// dropped past [`MAX_PENDING`].
    pub fn deliver(&self, url: &str) {
        let mut state = self.lock();
        if let Some(waiting) = state.waiting.take() {
            state.deliveries += 1;
            let _ = waiting.send(Ok(url.to_owned()));
            return;
        }
        if state.queued.len() >= MAX_PENDING {
            state.queued.pop_front();
        }
        state.queued.push_back(url.to_owned());
    }

    /// Makes the next `next_url` fail with `error` — a waiting one at once, else the next
    /// call, before any queued link is handed over.
    pub fn fail_next(&self, error: DeepLinksError) {
        let mut state = self.lock();
        if let Some(waiting) = state.waiting.take() {
            let _ = waiting.send(Err(error));
            return;
        }
        state.next_error = Some(error);
    }

    /// Sets what `web_link_kind` answers; `None` by default, as a platform without verified
    /// web links.
    pub fn set_web_link_kind(&self, kind: Option<LinkKind>) {
        self.lock().web_kind = kind;
    }

    /// Whether a `next_url` call waits for its answer.
    pub fn waiting(&self) -> bool {
        self.lock().waiting.is_some()
    }

    /// The links waiting in the queue.
    pub fn queued(&self) -> Vec<String> {
        self.lock().queued.iter().cloned().collect()
    }

    /// How many links were handed to a waiting call or taken from the queue.
    pub fn deliveries(&self) -> usize {
        self.lock().deliveries
    }

    /// How many times a wait was asked to cancel.
    pub fn cancels(&self) -> usize {
        self.lock().cancels
    }

    /// How many times the initial link was read.
    pub fn initial_reads(&self) -> usize {
        self.lock().initial_reads
    }
}

impl Backend for FakeBackend {
    fn initial_url(&self) -> BackendFuture<'_, Result<Option<String>, DeepLinksError>> {
        let mut state = self.lock();
        state.initial_reads += 1;
        let initial = state.initial.clone();
        Box::pin(ready(Ok(initial)))
    }

    fn next_url(&self) -> BackendFuture<'_, Result<String, DeepLinksError>> {
        let mut state = self.lock();
        if let Some(error) = state.next_error.take() {
            return Box::pin(ready(Err(error)));
        }
        if let Some(url) = state.queued.pop_front() {
            state.deliveries += 1;
            return Box::pin(ready(Ok(url)));
        }
        let (sender, receiver) = oneshot::channel();
        state.waiting = Some(sender);
        // A dropped receiver means the caller went away without cancelling: that is a cancel.
        Box::pin(async move {
            receiver
                .await
                .unwrap_or_else(|_| Err(DeepLinksError::new(ErrorKind::Cancelled)))
        })
    }

    fn cancel_wait(&self) {
        let mut state = self.lock();
        state.cancels += 1;
        if let Some(waiting) = state.waiting.take() {
            let _ = waiting.send(Err(DeepLinksError::new(ErrorKind::Cancelled)));
        }
    }

    fn web_link_kind(&self) -> Option<LinkKind> {
        self.lock().web_kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run<F: std::future::Future>(future: F) -> F::Output {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .ok()
            .expect("a runtime")
            .block_on(future)
    }

    #[test]
    fn the_initial_link_is_reported_once_set() {
        let fake = FakeBackend::new();
        assert_eq!(run(fake.initial_url()).ok(), Some(None));
        fake.set_initial(Some("myapp://open".to_owned()));
        assert_eq!(
            run(fake.initial_url()).ok(),
            Some(Some("myapp://open".to_owned()))
        );
        assert_eq!(fake.initial_reads(), 2);
    }

    #[test]
    fn a_delivery_reaches_the_waiting_call_at_once() {
        let fake = FakeBackend::new();
        let waiting = fake.next_url();
        fake.deliver("myapp://open");
        assert_eq!(run(waiting).ok(), Some("myapp://open".to_owned()));
        assert_eq!(fake.deliveries(), 1);
        assert_eq!(fake.queued(), Vec::<String>::new());
    }

    #[test]
    fn a_delivery_with_no_wait_queues_up_to_the_core_limit() {
        let fake = FakeBackend::new();
        fake.set_initial(None);
        for i in 0..=MAX_PENDING {
            fake.deliver(&format!("myapp://{i}"));
        }
        // One past the limit: the oldest went.
        assert_eq!(fake.queued().len(), MAX_PENDING);
        assert_eq!(
            run(fake.next_url()).ok(),
            Some("myapp://1".to_owned()),
            "the oldest was dropped"
        );
    }

    #[test]
    fn a_cancelled_wait_resolves_once_with_the_code() {
        let fake = FakeBackend::new();
        let waiting = fake.next_url();
        assert!(fake.waiting());
        fake.cancel_wait();
        let answer = run(waiting).err();
        assert_eq!(
            answer.map(DeepLinksError::code),
            Some("ERR_DEEP_LINKS_CANCELLED")
        );
        // Cancelling again touches nothing: the wait is gone.
        fake.cancel_wait();
        assert_eq!(fake.cancels(), 2);
    }

    #[test]
    fn the_next_answer_fails_before_the_queue_hands_over() {
        let fake = FakeBackend::new();
        fake.deliver("myapp://queued");
        fake.fail_next(DeepLinksError::new(ErrorKind::Timeout));
        assert_eq!(
            run(fake.next_url()).err().map(DeepLinksError::code),
            Some("ERR_DEEP_LINKS_TIMEOUT")
        );
        assert_eq!(run(fake.next_url()).ok(), Some("myapp://queued".to_owned()));
    }

    #[test]
    fn the_web_link_kind_is_what_the_test_set() {
        let fake = FakeBackend::new();
        assert_eq!(fake.web_link_kind(), None);
        fake.set_web_link_kind(Some(LinkKind::App));
        assert_eq!(fake.web_link_kind(), Some(LinkKind::App));
    }
}
