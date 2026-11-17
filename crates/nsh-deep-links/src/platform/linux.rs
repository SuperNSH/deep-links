//! Linux: custom-scheme links only, through NSH core, which NSH's Linux platform feeds the
//! URLs of the schemes `nsh.json` claims. The desktop has no verified-web-link contract, so an
//! `https` link is `ERR_DEEP_LINKS_UNSUPPORTED_PLATFORM`.

use crate::{
    backend::{Backend, BackendFuture},
    error::DeepLinksError,
    inbox::Inbox,
    links::LinkKind,
};

use super::core_links;

/// Only carries cancels: nothing on the desktop delivers a verified link into it.
static CANCELS: Inbox = Inbox::new();

pub(crate) struct NativeBackend;

impl Backend for NativeBackend {
    fn initial_url(&self) -> BackendFuture<'_, Result<Option<String>, DeepLinksError>> {
        Box::pin(async { Ok(core_links::launch_url().await) })
    }

    fn next_url(&self) -> BackendFuture<'_, Result<String, DeepLinksError>> {
        core_links::next_with(&CANCELS, false)
    }

    fn cancel_wait(&self) {
        CANCELS.cancel();
    }

    fn web_link_kind(&self) -> Option<LinkKind> {
        None
    }
}
