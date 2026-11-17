//! A backend whose deliveries the test scripts: the launch link, links while the app runs,
//! duplicates, platform failures and cancels. Tests use it, and so can an app that runs without
//! a platform behind it.

use std::{
    future::ready,
    sync::{Arc, Mutex, MutexGuard, PoisonError},
};

use crate::{
    backend::{Backend, BackendFuture},
    error::{DeepLinksError, ErrorKind},
    inbox::Inbox,
    links::LinkKind,
};

/// A backend with a scripted launch link and a queue of deliveries. Clones share one state.
///
/// It keeps the platforms' rules: a cancelled wait resolves `ERR_DEEP_LINKS_CANCELLED`, a
/// delivery with no wait is held, the oldest dropped past [`crate::MAX_PENDING`], and the same
/// link fired twice while the first copy resolves is refused as `ERR_DEEP_LINKS_BUSY`.
#[derive(Clone)]
pub struct FakeBackend {
    shared: Arc<Shared>,
}

struct Shared {
    inbox: Inbox,
    state: Mutex<State>,
}

#[derive(Default)]
struct State {
    initial: Option<String>,
    initial_error: Option<DeepLinksError>,
    web_kind: Option<LinkKind>,
    initial_reads: usize,
}

impl Default for FakeBackend {
    /// A backend with nothing scripted: no launch link, no deliveries, no verified web links.
    fn default() -> Self {
        Self::new()
    }
}

impl FakeBackend {
    /// A backend with nothing scripted.
    pub fn new() -> Self {
        Self {
            shared: Arc::new(Shared {
                inbox: Inbox::new(),
                state: Mutex::new(State::default()),
            }),
        }
    }

    fn lock(&self) -> MutexGuard<'_, State> {
        self.shared
            .state
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
    }

    /// Sets the URL the app was started with.
    pub fn set_initial(&self, url: Option<&str>) {
        self.lock().initial = url.map(str::to_owned);
    }

    /// Makes reading the launch URL fail with `error`, as a platform whose bridge broke.
    pub fn fail_initial(&self, error: DeepLinksError) {
        self.lock().initial_error = Some(error);
    }

    /// Hands a URL over: to the waiting call at once, else to the queue. `Err(Busy)` when it is
    /// a copy of the link the last wait resolved with.
    pub fn deliver(&self, url: &str) -> Result<(), DeepLinksError> {
        self.shared.inbox.deliver(url)
    }

    /// Makes the waiting `next_url` fail with `error`, else the next call, before any queued
    /// link: a platform failure (`ERR_DEEP_LINKS_BACKEND`), a deadline, a system cancel.
    pub fn fail_next(&self, error: DeepLinksError) {
        self.shared.inbox.fail(error);
    }

    /// Fails the waiting `next_url` as the platform's bridge would: `ERR_DEEP_LINKS_BACKEND`.
    pub fn fail_platform(&self) {
        self.fail_next(
            DeepLinksError::new(ErrorKind::Backend).with_detail("the scripted platform failed"),
        );
    }

    /// Sets what `web_link_kind` answers; `None` by default, as a platform without verified
    /// web links.
    pub fn set_web_link_kind(&self, kind: Option<LinkKind>) {
        self.lock().web_kind = kind;
    }

    /// Whether a `next_url` call waits for its answer.
    pub fn waiting(&self) -> bool {
        self.shared.inbox.waiting()
    }

    /// The links held for the next waits, oldest first.
    pub fn queued(&self) -> Vec<String> {
        self.shared.inbox.queued()
    }

    /// How many links were handed to a waiting call or taken from the queue.
    pub fn deliveries(&self) -> usize {
        self.shared.inbox.deliveries()
    }

    /// How many times a wait was asked to cancel.
    pub fn cancels(&self) -> usize {
        self.shared.inbox.cancels()
    }

    /// How many times the launch URL was read.
    pub fn initial_reads(&self) -> usize {
        self.lock().initial_reads
    }
}

impl Backend for FakeBackend {
    fn initial_url(&self) -> BackendFuture<'_, Result<Option<String>, DeepLinksError>> {
        let mut state = self.lock();
        state.initial_reads += 1;
        let answer = match state.initial_error.clone() {
            Some(error) => Err(error),
            None => Ok(state.initial.clone()),
        };
        Box::pin(ready(answer))
    }

    fn next_url(&self) -> BackendFuture<'_, Result<String, DeepLinksError>> {
        self.shared.inbox.next_url()
    }

    fn cancel_wait(&self) {
        self.shared.inbox.cancel();
    }

    fn web_link_kind(&self) -> Option<LinkKind> {
        self.lock().web_kind
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::MAX_PENDING;

    fn run<F: std::future::Future>(future: F) -> F::Output {
        match tokio::runtime::Builder::new_current_thread().build() {
            Ok(runtime) => runtime.block_on(future),
            Err(error) => panic!("no runtime: {error}"),
        }
    }

    fn code<T>(answer: Result<T, DeepLinksError>) -> Option<&'static str> {
        answer.err().map(|error| error.code())
    }

    #[test]
    fn the_initial_link_is_reported_once_set() {
        let fake = FakeBackend::new();
        assert_eq!(run(fake.initial_url()).ok(), Some(None));
        fake.set_initial(Some("myapp://open"));
        assert_eq!(
            run(fake.initial_url()).ok(),
            Some(Some("myapp://open".to_owned()))
        );
        assert_eq!(fake.initial_reads(), 2);
        fake.fail_initial(DeepLinksError::new(ErrorKind::Backend));
        assert_eq!(
            code(run(fake.initial_url())),
            Some("ERR_DEEP_LINKS_BACKEND")
        );
    }

    #[test]
    fn a_delivery_reaches_the_waiting_call_at_once() {
        let fake = FakeBackend::new();
        let waiting = fake.next_url();
        assert!(fake.waiting());
        assert!(fake.deliver("myapp://open").is_ok());
        assert_eq!(run(waiting).ok(), Some("myapp://open".to_owned()));
        assert_eq!(fake.deliveries(), 1);
        assert!(fake.queued().is_empty());
    }

    #[test]
    fn a_delivery_with_no_wait_queues_up_to_the_core_limit() {
        let fake = FakeBackend::new();
        for i in 0..=MAX_PENDING {
            assert!(fake.deliver(&format!("myapp://{i}")).is_ok());
        }
        assert_eq!(fake.queued().len(), MAX_PENDING);
        assert_eq!(run(fake.next_url()).ok(), Some("myapp://1".to_owned()));
    }

    #[test]
    fn a_copy_of_the_resolving_link_is_busy_and_another_link_queues() {
        let fake = FakeBackend::new();
        let waiting = fake.next_url();
        assert!(fake.deliver("myapp://a").is_ok());
        assert_eq!(code(fake.deliver("myapp://a")), Some("ERR_DEEP_LINKS_BUSY"));
        assert!(fake.deliver("myapp://b").is_ok());
        assert_eq!(run(waiting).ok(), Some("myapp://a".to_owned()));
        assert_eq!(fake.queued(), vec!["myapp://b".to_owned()]);
    }

    #[test]
    fn a_cancelled_wait_resolves_once_with_the_code() {
        let fake = FakeBackend::new();
        let waiting = fake.next_url();
        fake.cancel_wait();
        assert_eq!(code(run(waiting)), Some("ERR_DEEP_LINKS_CANCELLED"));
        fake.cancel_wait();
        assert_eq!(fake.cancels(), 2);
    }

    #[test]
    fn the_next_answer_fails_before_the_queue_hands_over() {
        let fake = FakeBackend::new();
        assert!(fake.deliver("myapp://queued").is_ok());
        fake.fail_platform();
        assert_eq!(code(run(fake.next_url())), Some("ERR_DEEP_LINKS_BACKEND"));
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
