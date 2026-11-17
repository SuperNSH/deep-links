//! Apple: Universal Links. The system hands one to the app as an `NSUserActivity` of type
//! `NSUserActivityTypeBrowsingWeb` through `continueUserActivity`, which NSH's hosts do not
//! implement (they hand core only `openURLs` / `openURLContexts`, the custom schemes). So this
//! module adds that one method to the host's delegate class at runtime, on the main queue: the
//! scene delegate on iOS, the application delegate on macOS. A class that already implements it
//! is left alone. Custom schemes come through NSH core.
//!
//! On iOS a Universal Link that cold-starts the app sits in the scene's connection options,
//! which the host reads before any crate code runs: it is not seen. On macOS the system calls
//! `continueUserActivity` after launch, so a launch link arrives through `next`.

use std::sync::atomic::{AtomicBool, Ordering};

use objc2::{
    msg_send,
    rc::Retained,
    runtime::{AnyClass, AnyObject, Imp, Sel},
    sel,
};
use objc2_foundation::{NSString, NSURL};

use crate::{
    backend::{Backend, BackendFuture},
    error::DeepLinksError,
    inbox::Inbox,
    links::LinkKind,
};

use super::core_links;

/// Universal Links the delegate method hands over.
static VERIFIED: Inbox = Inbox::new();
static INSTALLED: AtomicBool = AtomicBool::new(false);

pub(crate) struct NativeBackend;

impl Backend for NativeBackend {
    fn initial_url(&self) -> BackendFuture<'_, Result<Option<String>, DeepLinksError>> {
        install();
        Box::pin(async { Ok(core_links::launch_url().await) })
    }

    fn next_url(&self) -> BackendFuture<'_, Result<String, DeepLinksError>> {
        install();
        core_links::next_with(&VERIFIED, true)
    }

    fn cancel_wait(&self) {
        VERIFIED.cancel();
    }

    fn web_link_kind(&self) -> Option<LinkKind> {
        Some(LinkKind::Universal)
    }
}

/// Teaches the delegate `continueUserActivity`, once; retried on the next call while no delegate
/// exists yet (an iOS scene connects after launch).
fn install() {
    if INSTALLED.load(Ordering::Acquire) {
        return;
    }
    dispatch2::DispatchQueue::main().exec_async(|| {
        if !INSTALLED.load(Ordering::Acquire) && install_on_main() {
            INSTALLED.store(true, Ordering::Release);
        }
    });
}

/// Hands the activity's web page URL over, when it is a Universal Link.
fn take(activity: *mut AnyObject) {
    // SAFETY: the system passes a live `NSUserActivity` or nil.
    let Some(activity) = (unsafe { activity.as_ref() }) else {
        return;
    };
    // SAFETY: `activityType` and `webpageURL` are documented `NSUserActivity` properties.
    let kind: Option<Retained<NSString>> = unsafe { msg_send![activity, activityType] };
    if kind.is_none_or(|kind| kind.to_string() != "NSUserActivityTypeBrowsingWeb") {
        return;
    }
    // SAFETY: as above.
    let url: Option<Retained<NSURL>> = unsafe { msg_send![activity, webpageURL] };
    let text = url
        .and_then(|url| url.absoluteString())
        .map(|s| s.to_string());
    if let Some(text) = text {
        // A copy of the link the last wait resolved with is the system firing twice: dropped.
        let _ = VERIFIED.deliver(&text);
    }
}

/// Adds `imp` as `sel` to the class of `delegate` unless the class answers it already. `true`
/// when the class now has it.
fn add_method(delegate: &AnyObject, sel: Sel, imp: Imp, types: &std::ffi::CStr) -> bool {
    let class: &AnyClass = delegate.class();
    if class.instance_method(sel).is_some() {
        return true;
    }
    let class = std::ptr::from_ref(class).cast_mut();
    // SAFETY: `imp` has the C signature `types` describes, and the class is a registered one.
    unsafe { objc2::ffi::class_addMethod(class, sel, imp, types.as_ptr()).as_bool() }
}

#[cfg(target_os = "macos")]
fn install_on_main() -> bool {
    use objc2::{Encode, runtime::Bool};

    unsafe extern "C-unwind" fn continue_user_activity(
        _this: *mut AnyObject,
        _cmd: Sel,
        _app: *mut AnyObject,
        activity: *mut AnyObject,
        _restoration: *mut AnyObject,
    ) -> Bool {
        take(activity);
        Bool::YES
    }
    let Some(class) = AnyClass::get(c"NSApplication") else {
        return false;
    };
    // SAFETY: `sharedApplication` and `delegate` are documented, on the main thread.
    let app: Option<Retained<AnyObject>> = unsafe { msg_send![class, sharedApplication] };
    let Some(app) = app else { return false };
    let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![&*app, delegate] };
    let Some(delegate) = delegate else {
        return false;
    };
    let types = format!("{}@:@@@?", Bool::ENCODING);
    let Ok(types) = std::ffi::CString::new(types) else {
        return false;
    };
    // SAFETY: the function matches `application:continueUserActivity:restorationHandler:`.
    let imp = unsafe {
        std::mem::transmute::<
            unsafe extern "C-unwind" fn(
                *mut AnyObject,
                Sel,
                *mut AnyObject,
                *mut AnyObject,
                *mut AnyObject,
            ) -> Bool,
            Imp,
        >(continue_user_activity)
    };
    add_method(
        &delegate,
        sel!(application:continueUserActivity:restorationHandler:),
        imp,
        &types,
    )
}

#[cfg(not(target_os = "macos"))]
fn install_on_main() -> bool {
    use objc2_foundation::NSSet;

    unsafe extern "C-unwind" fn continue_user_activity(
        _this: *mut AnyObject,
        _cmd: Sel,
        _scene: *mut AnyObject,
        activity: *mut AnyObject,
    ) {
        take(activity);
    }
    let Some(class) = AnyClass::get(c"UIApplication") else {
        return false;
    };
    // SAFETY: `sharedApplication` and `connectedScenes` are documented, on the main thread.
    let app: Option<Retained<AnyObject>> = unsafe { msg_send![class, sharedApplication] };
    let Some(app) = app else { return false };
    let scenes: Option<Retained<NSSet<AnyObject>>> = unsafe { msg_send![&*app, connectedScenes] };
    let Some(scenes) = scenes else { return false };
    // SAFETY: the function matches `scene:continueUserActivity:`.
    let imp = unsafe {
        std::mem::transmute::<
            unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject),
            Imp,
        >(continue_user_activity)
    };
    let mut installed = false;
    for scene in scenes.iter() {
        // SAFETY: `UIScene.delegate` is documented.
        let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![&*scene, delegate] };
        if let Some(delegate) = delegate {
            installed |= add_method(&delegate, sel!(scene:continueUserActivity:), imp, c"v@:@@");
        }
    }
    installed
}
