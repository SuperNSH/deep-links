//! The web: the page's own address when the app loaded is the one link, read from NSH core
//! (`UrlScheme.url()`). A browser tab gets no OS hand-off while it runs, so a wait is refused at
//! once with `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM`; there are no custom schemes in a tab.

use std::future::ready;

use crate::{
    backend::{Backend, BackendFuture},
    error::{DeepLinksError, ErrorKind},
    links::LinkKind,
};

use super::core_links;

pub(crate) struct NativeBackend;

impl Backend for NativeBackend {
    fn initial_url(&self) -> BackendFuture<'_, Result<Option<String>, DeepLinksError>> {
        Box::pin(async { Ok(core_links::launch_url().await) })
    }

    fn next_url(&self) -> BackendFuture<'_, Result<String, DeepLinksError>> {
        Box::pin(ready(Err(DeepLinksError::new(
            ErrorKind::UnsupportedPlatform,
        )
        .with_detail(
            "a web page gets no links while it runs; its address is `initial`",
        ))))
    }

    fn cancel_wait(&self) {}

    fn web_link_kind(&self) -> Option<LinkKind> {
        Some(LinkKind::Web)
    }
}
