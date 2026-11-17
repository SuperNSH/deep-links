//! Deep links for Native Shell apps: Universal Links (iOS, macOS), App Links (Android) and the
//! app's custom schemes, checked and routed to screens.
//!
//! [`DeepLinks`] follows `app_links`: [`initial`](DeepLinks::initial) is the link that started
//! the app, [`next`](DeepLinks::next) the next one, each resolving exactly once with a [`Link`]
//! or a [`DeepLinksError`] with a stable code. Custom schemes come through NSH core's
//! `url_scheme`, whose rules and limits this crate re-exports rather than copies. [`RouteTable`]
//! ports React Navigation's linking config; [`association`] builds and checks the files the web
//! serves to vouch for the app.

pub mod association;
mod backend;
mod error;
mod fake;
mod inbox;
mod links;
mod native;
pub mod platform;
mod routes;

use nsh_core::prelude::*;

pub use backend::{Backend, BackendFuture};
pub use error::{DeepLinksError, ErrorKind};
pub use fake::FakeBackend;
pub use links::{DeepLinks, Link, LinkKind, PendingLink};
pub use native::NativeDeepLinks;
/// NSH core's limits, re-exported so a change in core is a change here.
pub use nsh_core::url_scheme::{MAX_PENDING, MAX_URL_LEN};
pub use routes::{ParamType, RouteMatch, RouteTable};

/// The crate's native module: what TSX imports from `nsh-deep-links/native`.
#[native_module("nsh-deep-links/native")]
fn native(registry: &mut Registry) {
    registry.class::<NativeDeepLinks>();
}
