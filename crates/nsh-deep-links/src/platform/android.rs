//! Android: App Links. The system starts the activity with an `ACTION_VIEW` intent whose data is
//! the verified `https` URL (an `android:autoVerify` intent filter, checked against the host's
//! `assetlinks.json`). The launch intent is read over JNI for `initial`. Custom schemes come
//! through NSH core.
//!
//! An App Link that reaches a running app goes to the activity's `onNewIntent`, which NSH's
//! activity hands only to core; core keeps the schemes `nsh.json` claims, and `https` is never
//! one. So a verified link while the app runs is not seen here: that needs the host to pass
//! `https` intents on, which a crate cannot add.
//!
//! `ndk-context` must hold the activity (NSH's host installs it); a context that is not an
//! `Activity` has no launch intent, and reads as no link.

use std::sync::OnceLock;

use jni::{
    Env, JValueOwned, JavaVM, jni_sig, jni_str,
    objects::{JObject, JString},
    signature::MethodSignature,
    strings::JNIStr,
    sys::jobject,
};

use crate::{
    backend::{Backend, BackendFuture},
    error::{DeepLinksError, ErrorKind},
    inbox::Inbox,
    links::LinkKind,
};

use super::core_links;

const ACTION_VIEW: &str = "android.intent.action.VIEW";

/// Only carries cancels: no runtime App Link reaches this crate.
static CANCELS: Inbox = Inbox::new();

pub(crate) struct NativeBackend;

impl Backend for NativeBackend {
    fn initial_url(&self) -> BackendFuture<'_, Result<Option<String>, DeepLinksError>> {
        Box::pin(async {
            if let Some(url) = launch_app_link().await? {
                return Ok(Some(url));
            }
            Ok(core_links::launch_url().await)
        })
    }

    fn next_url(&self) -> BackendFuture<'_, Result<String, DeepLinksError>> {
        core_links::next_with(&CANCELS, false)
    }

    fn cancel_wait(&self) {
        CANCELS.cancel();
    }

    fn web_link_kind(&self) -> Option<LinkKind> {
        Some(LinkKind::App)
    }
}

fn backend_error() -> DeepLinksError {
    DeepLinksError::new(ErrorKind::Backend)
}

impl From<jni::errors::Error> for DeepLinksError {
    fn from(_: jni::errors::Error) -> Self {
        backend_error()
    }
}

/// The VM and context `ndk-context` holds.
#[derive(Clone, Copy)]
struct Ptrs {
    vm: *mut jni::sys::JavaVM,
    context: jobject,
}

// SAFETY: ndk-context keeps the VM and the context's global reference for the whole process;
// these are read-only pointers to them, valid from any thread.
unsafe impl Send for Ptrs {}
unsafe impl Sync for Ptrs {}

static ANDROID: OnceLock<Option<Ptrs>> = OnceLock::new();

fn pointers() -> Option<Ptrs> {
    *ANDROID.get_or_init(|| {
        // The getter panics while nothing is installed; a silent hook keeps that out of the log.
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(|_| {}));
        let probe = std::panic::catch_unwind(ndk_context::android_context);
        std::panic::set_hook(previous);
        probe.ok().map(|context| Ptrs {
            vm: context.vm().cast(),
            context: context.context().cast(),
        })
    })
}

/// The launch intent's URL when it is an `ACTION_VIEW` of an `http(s)` URL; `None` for any other
/// launch, or without an activity.
async fn launch_app_link() -> Result<Option<String>, DeepLinksError> {
    let Some(ptrs) = pointers() else {
        return Ok(None);
    };
    nsh_core::prelude::blocking(move || read_launch_intent(ptrs))
        .await
        .map_err(|_| backend_error())?
}

fn read_launch_intent(ptrs: Ptrs) -> Result<Option<String>, DeepLinksError> {
    // SAFETY: the VM pointer came from ndk-context, which keeps the VM for the process; the
    // wrapper only borrows it.
    let vm = unsafe { JavaVM::from_raw(ptrs.vm) };
    vm.attach_current_thread(|env| -> Result<Option<String>, DeepLinksError> {
        // SAFETY: ndk-context holds this global reference for the process; it is only borrowed.
        let context = unsafe { JObject::from_raw(env, ptrs.context) };
        env.with_local_frame(16, |env| {
            let activity = env
                .is_instance_of(&context, jni_str!("android/app/Activity"))
                .map_err(|_| {
                    env.exception_clear();
                    backend_error()
                })?;
            if !activity {
                return Ok(None);
            }
            let intent = call(
                env,
                &context,
                jni_str!("getIntent"),
                jni_sig!("()Landroid/content/Intent;"),
            )?
            .l()
            .map_err(|_| backend_error())?;
            if intent.is_null() {
                return Ok(None);
            }
            let action = string_of(
                env,
                &intent,
                jni_str!("getAction"),
                jni_sig!("()Ljava/lang/String;"),
            )?;
            if action.as_deref() != Some(ACTION_VIEW) {
                return Ok(None);
            }
            let data = string_of(
                env,
                &intent,
                jni_str!("getDataString"),
                jni_sig!("()Ljava/lang/String;"),
            )?;
            Ok(data.filter(|url| {
                let lower = url.to_ascii_lowercase();
                lower.starts_with("https:") || lower.starts_with("http:")
            }))
        })
    })
}

/// Calls a no-argument method, clearing the Java exception when it throws.
fn call<'local>(
    env: &mut Env<'local>,
    obj: &JObject<'_>,
    name: &JNIStr,
    sig: MethodSignature<'static, 'static>,
) -> Result<JValueOwned<'local>, DeepLinksError> {
    env.call_method(obj, name, sig, &[]).map_err(|_| {
        env.exception_clear();
        backend_error()
    })
}

/// A no-argument method answering a `String`; `None` for `null`.
fn string_of(
    env: &mut Env<'_>,
    obj: &JObject<'_>,
    name: &JNIStr,
    sig: MethodSignature<'static, 'static>,
) -> Result<Option<String>, DeepLinksError> {
    let value = call(env, obj, name, sig)?
        .l()
        .map_err(|_| backend_error())?;
    if value.is_null() {
        return Ok(None);
    }
    let text: JString<'_> = env
        .cast_local::<JString>(value)
        .map_err(|_| backend_error())?;
    let chars = text.mutf8_chars(env).map_err(|_| {
        env.exception_clear();
        backend_error()
    })?;
    Ok(Some(String::from(chars)))
}
