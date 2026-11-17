//! A platform with no link intake: no launch link, and every wait refused at once with
//! `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM`, so nothing hangs. Built on every target; `mod.rs`
//! wires it in only where no real backend exists.

use std::future::ready;

use crate::{
    backend::{Backend, BackendFuture},
    error::{DeepLinksError, ErrorKind},
    links::LinkKind,
};

/// The backend of a platform that delivers no links. A verified web link resolved through it
/// is `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM`, as is every `next`.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnsupportedBackend;

impl Backend for UnsupportedBackend {
    fn initial_url(&self) -> BackendFuture<'_, Result<Option<String>, DeepLinksError>> {
        Box::pin(ready(Ok(None)))
    }

    fn next_url(&self) -> BackendFuture<'_, Result<String, DeepLinksError>> {
        Box::pin(ready(Err(DeepLinksError::new(
            ErrorKind::UnsupportedPlatform,
        )
        .with_detail("this platform delivers no links"))))
    }

    fn cancel_wait(&self) {}

    fn web_link_kind(&self) -> Option<LinkKind> {
        None
    }
}
