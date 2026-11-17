//! Custom-scheme links through NSH core's `UrlScheme`, which every NSH host already feeds: the
//! platform hands core each URL the system opens the app with, core keeps those of the schemes
//! `nsh.json` claims, and `next_url_scheme_open` / `app_url` read them. This crate adds no scheme
//! intake of its own.
//!
//! Core's answers live on the engine's thread, so the state lives in a surface global
//! (`try_global`), which is also how this module tells it runs outside an engine: there is none.

use std::{cell::RefCell, rc::Rc};

use nsh_core::element::{app_url, try_global};

#[derive(Default)]
struct State {
    /// The launch link, read once: `Some(None)` when there was none.
    launch: Option<Option<String>>,
    /// Core's wait, kept across our cancels: core holds one answer slot, and a link it
    /// delivered into a dropped answer would be lost.
    #[cfg(not(target_arch = "wasm32"))]
    parked: Option<nsh_core::element::Answer<String>>,
    /// The launch link, which core also queued for the first listener: skipped once, so `next`
    /// never hands out what `initial` did.
    #[cfg(not(target_arch = "wasm32"))]
    skip: Option<String>,
}

#[derive(Default, Clone)]
struct Slot(Rc<RefCell<State>>);

/// This engine's state; `None` outside an engine.
fn state() -> Option<Rc<RefCell<State>>> {
    try_global::<Slot>().map(|global| global.with_untracked(|slot| Rc::clone(&slot.0)))
}

/// The link that started the app: core's address the first time deep links are read. Core sets
/// its address from each accepted link, so that is the launch link, or the first link that came
/// before anything read. `None` outside an engine.
pub(super) async fn launch_url() -> Option<String> {
    let state = state()?;
    if let Some(read) = state.borrow().launch.clone() {
        return read;
    }
    let url = app_url().await;
    let mut state = state.borrow_mut();
    if let Some(read) = state.launch.clone() {
        // Another read finished first; its answer stands.
        return read;
    }
    state.launch = Some(url.clone());
    #[cfg(not(target_arch = "wasm32"))]
    state.skip.clone_from(&url);
    url
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) use runtime::next_with;

#[cfg(not(target_arch = "wasm32"))]
mod runtime {
    use std::{
        cell::RefCell,
        future::Future,
        pin::Pin,
        rc::Rc,
        task::{Context, Poll},
    };

    use nsh_core::element::next_url_scheme_open;

    use super::{State, launch_url, state};
    use crate::{
        backend::BackendFuture,
        error::{DeepLinksError, ErrorKind},
        inbox::Inbox,
    };

    /// The next custom-scheme link from core.
    struct CoreNext(Rc<RefCell<State>>);

    impl Future for CoreNext {
        type Output = String;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<String> {
            loop {
                let parked = self.0.borrow_mut().parked.take();
                let mut answer = parked.unwrap_or_else(next_url_scheme_open);
                match Pin::new(&mut answer).poll(cx) {
                    Poll::Pending => {
                        self.0.borrow_mut().parked = Some(answer);
                        return Poll::Pending;
                    }
                    Poll::Ready(url) => {
                        let mut state = self.0.borrow_mut();
                        if state.skip.take_if(|launch| *launch == url).is_none() {
                            return Poll::Ready(url);
                        }
                    }
                }
            }
        }
    }

    /// The next link from either source: core's custom schemes, or `verified`, the platform's
    /// own intake of verified web links. `stop` cancels through the inbox. Outside an engine
    /// only the inbox can answer, and a platform without one answers `ERR_DEEP_LINKS_BACKEND` at
    /// once rather than wait for links that cannot come.
    pub(crate) fn next_with(
        verified: &'static Inbox,
        has_verified: bool,
    ) -> BackendFuture<'static, Result<String, DeepLinksError>> {
        let from_inbox = verified.next_url();
        Box::pin(async move {
            let Some(core) = state() else {
                if has_verified {
                    return from_inbox.await;
                }
                return Err(DeepLinksError::new(ErrorKind::Backend).with_detail(
                    "custom-scheme links come through an NSH engine, and none runs here",
                ));
            };
            // Reading the launch link first arms the skip of its queued copy.
            launch_url().await;
            tokio::select! {
                url = CoreNext(core) => Ok(url),
                answer = from_inbox => answer,
            }
        })
    }
}
