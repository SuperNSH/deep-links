//! Apple: Universal Links. The system hands one to the app as an `NSUserActivity` of type
//! `NSUserActivityTypeBrowsingWeb` through `continueUserActivity`. NSH 0.7.1's hosts implement
//! that method for Handoff and web links alike (`nsh-platform-ios/src/app.rs`,
//! `nsh-platform-macos/src/app.rs`) and pass a web link on to core. This module still puts itself
//! in front of the host's delegate method at runtime, on the main queue: the scene delegate on
//! iOS, the application delegate on macOS. It takes the link into its inbox, then the host's own
//! implementation runs as before, after ours. A delegate without the method gets ours alone.
//! Custom schemes come through NSH core.
//!
//! On iOS a Universal Link that cold-starts the app sits in the scene's connection options,
//! which the host reads before any crate code runs and passes to core: it is not seen here. On
//! macOS the system calls `continueUserActivity` after launch, so a launch link arrives through
//! `next`.

use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

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
/// The delegate's own implementation of the method, replaced by ours; 0 while it had none.
/// One delegate class carries it: NSH's hosts have one scene or application delegate.
static ORIGINAL: AtomicUsize = AtomicUsize::new(0);

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

/// Hands the activity's web page URL over, when it is a Universal Link. `true` when it was one.
fn take(activity: *mut AnyObject) -> bool {
    // SAFETY: the system passes a live `NSUserActivity` or nil.
    let Some(activity) = (unsafe { activity.as_ref() }) else {
        return false;
    };
    // SAFETY: `activityType` and `webpageURL` are documented `NSUserActivity` properties.
    let kind: Option<Retained<NSString>> = unsafe { msg_send![activity, activityType] };
    if kind.is_none_or(|kind| kind.to_string() != "NSUserActivityTypeBrowsingWeb") {
        return false;
    }
    // SAFETY: as above.
    let url: Option<Retained<NSURL>> = unsafe { msg_send![activity, webpageURL] };
    let text = url
        .and_then(|url| url.absoluteString())
        .map(|s| s.to_string());
    let Some(text) = text else {
        return false;
    };
    // A copy of the link the last wait resolved with is the system firing twice: dropped.
    let _ = VERIFIED.deliver(&text);
    true
}

/// The delegate's replaced implementation, when it had one.
fn original() -> Option<Imp> {
    match ORIGINAL.load(Ordering::Acquire) {
        0 => None,
        // SAFETY: `ORIGINAL` holds an `Imp` that `chain_method` stored, never another value.
        raw => Some(unsafe { std::mem::transmute::<usize, Imp>(raw) }),
    }
}

/// Puts `imp` in front of `sel` on the class of `delegate`: a class without the method gets
/// `imp`, and a class with one gets `imp` in its place, the old implementation kept in
/// [`ORIGINAL`] for `imp` to call. `true` when the class now answers through `imp`.
fn chain_method(delegate: &AnyObject, sel: Sel, imp: Imp, types: &std::ffi::CStr) -> bool {
    let class: &AnyClass = delegate.class();
    if let Some(method) = class.instance_method(sel) {
        if method.implementation() as usize == imp as usize {
            return true;
        }
        // SAFETY: `imp` has the signature of `sel`, as the implementation it replaces has.
        let previous = unsafe { method.set_implementation(imp) };
        ORIGINAL.store(previous as usize, Ordering::Release);
        return true;
    }
    let class = std::ptr::from_ref(class).cast_mut();
    // SAFETY: `imp` has the C signature `types` describes, and the class is a registered one.
    unsafe { objc2::ffi::class_addMethod(class, sel, imp, types.as_ptr()).as_bool() }
}

/// `application:continueUserActivity:restorationHandler:`, with this module's handling first.
#[cfg(target_os = "macos")]
unsafe extern "C-unwind" fn macos_continue_user_activity(
    this: *mut AnyObject,
    cmd: Sel,
    app: *mut AnyObject,
    activity: *mut AnyObject,
    restoration: *mut AnyObject,
) -> objc2::runtime::Bool {
    type Method = unsafe extern "C-unwind" fn(
        *mut AnyObject,
        Sel,
        *mut AnyObject,
        *mut AnyObject,
        *mut AnyObject,
    ) -> objc2::runtime::Bool;
    let taken = take(activity);
    let handled = original().is_some_and(|original| {
        // SAFETY: the original implements the same selector, so it has this signature.
        let original = unsafe { std::mem::transmute::<Imp, Method>(original) };
        unsafe { original(this, cmd, app, activity, restoration) }.as_bool()
    });
    objc2::runtime::Bool::new(taken || handled)
}

/// Puts [`macos_continue_user_activity`] in front of the application delegate's method.
#[cfg(target_os = "macos")]
fn chain_macos(delegate: &AnyObject) -> bool {
    use objc2::Encode;

    let types = format!("{}@:@@@?", objc2::runtime::Bool::ENCODING);
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
            ) -> objc2::runtime::Bool,
            Imp,
        >(macos_continue_user_activity)
    };
    chain_method(
        delegate,
        sel!(application:continueUserActivity:restorationHandler:),
        imp,
        &types,
    )
}

#[cfg(target_os = "macos")]
fn install_on_main() -> bool {
    let Some(class) = AnyClass::get(c"NSApplication") else {
        return false;
    };
    // SAFETY: `sharedApplication` and `delegate` are documented, on the main thread.
    let app: Option<Retained<AnyObject>> = unsafe { msg_send![class, sharedApplication] };
    let Some(app) = app else { return false };
    let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![&*app, delegate] };
    delegate.is_some_and(|delegate| chain_macos(&delegate))
}

/// `scene:continueUserActivity:`, with this module's handling first.
#[cfg(not(target_os = "macos"))]
unsafe extern "C-unwind" fn scene_continue_user_activity(
    this: *mut AnyObject,
    cmd: Sel,
    scene: *mut AnyObject,
    activity: *mut AnyObject,
) {
    type Method = unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject);
    take(activity);
    if let Some(original) = original() {
        // SAFETY: the original implements the same selector, so it has this signature.
        let original = unsafe { std::mem::transmute::<Imp, Method>(original) };
        unsafe { original(this, cmd, scene, activity) };
    }
}

#[cfg(not(target_os = "macos"))]
fn install_on_main() -> bool {
    use objc2_foundation::NSSet;

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
        >(scene_continue_user_activity)
    };
    let mut installed = false;
    for scene in scenes.iter() {
        // SAFETY: `UIScene.delegate` is documented.
        let delegate: Option<Retained<AnyObject>> = unsafe { msg_send![&*scene, delegate] };
        if let Some(delegate) = delegate {
            installed |= chain_method(&delegate, sel!(scene:continueUserActivity:), imp, c"v@:@@");
        }
    }
    installed
}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    use std::sync::atomic::{AtomicBool, Ordering};

    use objc2::{
        AnyThread, ClassType,
        rc::Retained,
        runtime::{AnyObject, Bool, ClassBuilder, Imp, NSObject, Sel},
        sel,
    };
    use objc2_foundation::{NSString, NSURL, NSUserActivity};

    use super::*;

    type Method = unsafe extern "C-unwind" fn(
        *mut AnyObject,
        Sel,
        *mut AnyObject,
        *mut AnyObject,
        *mut AnyObject,
    ) -> Bool;

    static HOST_CALLED: AtomicBool = AtomicBool::new(false);

    /// A host's own `continueUserActivity`, as NSH 0.7.1's macOS host has one.
    extern "C-unwind" fn host_method(
        _this: &AnyObject,
        _cmd: Sel,
        _app: *mut AnyObject,
        _activity: *mut AnyObject,
        _restoration: *mut AnyObject,
    ) -> Bool {
        HOST_CALLED.store(true, Ordering::SeqCst);
        Bool::NO
    }

    #[test]
    fn a_universal_link_reaches_the_inbox_while_the_hosts_own_method_still_runs() {
        let mut builder = ClassBuilder::new(c"DeepLinksTestDelegate", NSObject::class())
            .expect("the test class name is free");
        // SAFETY: `host_method` has the signature of the selector it is added for.
        unsafe {
            builder.add_method(
                sel!(application:continueUserActivity:restorationHandler:),
                // `_` for one lifetime: the method trait is not implemented for all of them.
                host_method as extern "C-unwind" fn(_, _, _, _, _) -> _,
            );
        }
        let class = builder.register();
        // SAFETY: `new` is NSObject's.
        let delegate: Retained<AnyObject> = unsafe { msg_send![class, new] };

        assert!(
            chain_macos(&delegate),
            "a delegate with the method is chained"
        );
        assert!(
            chain_macos(&delegate),
            "chaining again keeps the host's method as the original, not our own"
        );

        let kind = NSString::from_str("NSUserActivityTypeBrowsingWeb");
        let activity = NSUserActivity::initWithActivityType(NSUserActivity::alloc(), &kind);
        let page = NSURL::URLWithString(&NSString::from_str("https://example.com/orders/7"))
            .expect("a valid URL");
        activity.setWebpageURL(Some(&page));

        let selector = sel!(application:continueUserActivity:restorationHandler:);
        let imp: Imp = class
            .instance_method(selector)
            .expect("the class answers the selector")
            .implementation();
        // SAFETY: the selector's signature.
        let call = unsafe { std::mem::transmute::<Imp, Method>(imp) };
        // SAFETY: a live delegate and activity; the restoration handler is unused.
        let handled = unsafe {
            call(
                Retained::as_ptr(&delegate).cast_mut(),
                selector,
                std::ptr::null_mut(),
                Retained::as_ptr(&activity).cast_mut().cast::<AnyObject>(),
                std::ptr::null_mut(),
            )
        };

        assert!(handled.as_bool(), "a web page the module took is handled");
        assert!(
            HOST_CALLED.load(Ordering::SeqCst),
            "the host's own method still ran"
        );
        assert_eq!(
            VERIFIED.queued(),
            vec!["https://example.com/orders/7".to_owned()]
        );
    }
}
