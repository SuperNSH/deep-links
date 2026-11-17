//! The platform backend as TSX sees it: `NativeDeepLinks`, the raw link intake the TS
//! `rustBackend()` binds, plus NSH core's URL rules (`checkUrl`, `checkScheme`, the limits) so
//! the TS pipeline applies core's own rules rather than a copy. Rejections carry the `code`.

use std::sync::Arc;

use nsh_core::{prelude::*, url_scheme};

use crate::{backend::Backend, error::DeepLinksError, platform};

/// The platform's link intake. Every handle shares the platform's one wait.
#[class]
pub struct NativeDeepLinks {
    backend: Arc<dyn Backend>,
}

#[class]
impl NativeDeepLinks {
    /// The intake of the platform this build runs on.
    pub fn new() -> NativeDeepLinks {
        NativeDeepLinks {
            backend: platform::default_backend(),
        }
    }

    /// The URL the app was started with, unchecked; `null` when there was none.
    pub async fn initial_url(this: Instance<Self>) -> Result<Option<String>, JsError> {
        let backend = this.with(|native| Arc::clone(&native.backend));
        backend.initial_url().await.map_err(into_js)
    }

    /// The next URL the system hands the app, unchecked. Resolves once.
    pub async fn next_url(this: Instance<Self>) -> Result<String, JsError> {
        let backend = this.with(|native| Arc::clone(&native.backend));
        backend.next_url().await.map_err(into_js)
    }

    /// Resolves the waiting `nextUrl` with `ERR_DEEP_LINKS_CANCELLED`.
    pub fn cancel_wait(&self) {
        self.backend.cancel_wait();
    }

    /// `"universal"`, `"app"` or `"web"`: what this platform's verified `http(s)` links are;
    /// `null` where it has none.
    pub fn web_link_kind(&self) -> Option<String> {
        self.backend
            .web_link_kind()
            .map(|kind| kind.as_str().to_owned())
    }

    /// NSH core's `MAX_URL_LEN`: the longest link taken, in bytes.
    pub fn max_url_len() -> u32 {
        u32::try_from(url_scheme::MAX_URL_LEN).unwrap_or(u32::MAX)
    }

    /// NSH core's `MAX_PENDING`: the most links held while nothing waits.
    pub fn max_pending() -> u32 {
        u32::try_from(url_scheme::MAX_PENDING).unwrap_or(u32::MAX)
    }

    /// NSH core's `check_url`: `null` when `url` is a link of one of `schemes`, else why not.
    pub fn check_url(url: String, schemes: Vec<String>) -> Option<String> {
        url_scheme::check_url(&url, &schemes).err()
    }

    /// NSH core's `check_scheme`: `null` when an app may claim `scheme`, else why not.
    pub fn check_scheme(scheme: String) -> Option<String> {
        url_scheme::check_scheme(&scheme).err()
    }
}

impl Default for NativeDeepLinks {
    fn default() -> Self {
        Self::new()
    }
}

/// The rejection TSX sees: the message, and the `code` apps match on.
fn into_js(error: DeepLinksError) -> JsError {
    JsError::Error(error.message()).with_code(error.code())
}
