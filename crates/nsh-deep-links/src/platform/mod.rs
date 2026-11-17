//! The platform backends. Each target gets one `NativeBackend`; a target with none gets
//! [`unsupported::UnsupportedBackend`], which is public and built on every target so its coded
//! refusals are testable anywhere.

#[cfg(target_os = "android")]
mod android;
#[cfg(target_vendor = "apple")]
mod apple;
#[cfg(any(
    target_vendor = "apple",
    target_os = "android",
    target_os = "linux",
    target_os = "windows",
    target_arch = "wasm32"
))]
mod core_links;
#[cfg(target_os = "linux")]
mod linux;
pub mod unsupported;
#[cfg(target_arch = "wasm32")]
mod web;
#[cfg(target_os = "windows")]
mod windows;

use std::sync::Arc;

use crate::backend::Backend;

/// The backend of the target this build runs on.
pub(crate) fn default_backend() -> Arc<dyn Backend> {
    #[cfg(target_vendor = "apple")]
    return Arc::new(apple::NativeBackend);
    #[cfg(target_os = "android")]
    return Arc::new(android::NativeBackend);
    #[cfg(target_os = "linux")]
    return Arc::new(linux::NativeBackend);
    #[cfg(target_os = "windows")]
    return Arc::new(windows::NativeBackend);
    #[cfg(target_arch = "wasm32")]
    return Arc::new(web::NativeBackend);
    #[cfg(not(any(
        target_vendor = "apple",
        target_os = "android",
        target_os = "linux",
        target_os = "windows",
        target_arch = "wasm32"
    )))]
    return Arc::new(unsupported::UnsupportedBackend);
}
