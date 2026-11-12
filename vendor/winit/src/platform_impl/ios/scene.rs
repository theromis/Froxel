//! UIScene lifecycle (Froxel patch).
//!
//! iOS 27 stops an app built with the iOS 26 SDK or later at launch unless it adopts the
//! UIScene lifecycle. winit 0.30 creates its `UIWindow`s with `initWithFrame:`, which under that
//! lifecycle belong to no scene and are never shown. `WinitSceneDelegate` is the scene delegate
//! the app's `Info.plist` names in `UIApplicationSceneManifest`: when UIKit connects the scene,
//! every winit window created before then is moved into it and made key, and windows created
//! afterwards join it at creation.

use std::cell::RefCell;
use std::ffi::CStr;

use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{declare_class, msg_send, mutability, ClassType, DeclaredClass};
use objc2_foundation::{MainThreadMarker, NSObject, NSObjectProtocol};
use objc2_ui_kit::UIResponder;

use super::window::WinitUIWindow;

/// The class name `Info.plist` must give as `UISceneDelegateClassName`.
pub const SCENE_DELEGATE_CLASS: &str = "WinitSceneDelegate";

thread_local! {
    /// The connected window scene, once UIKit has connected it.
    static SCENE: RefCell<Option<Retained<AnyObject>>> = const { RefCell::new(None) };
    /// Windows created before the scene connected.
    static WAITING: RefCell<Vec<Retained<WinitUIWindow>>> = const { RefCell::new(Vec::new()) };
}

declare_class!(
    pub(crate) struct WinitSceneDelegate;

    unsafe impl ClassType for WinitSceneDelegate {
        #[inherits(NSObject)]
        type Super = UIResponder;
        type Mutability = mutability::MainThreadOnly;
        const NAME: &'static str = SCENE_DELEGATE_CLASS;
    }

    impl DeclaredClass for WinitSceneDelegate {}

    unsafe impl WinitSceneDelegate {
        #[method(scene:willConnectToSession:options:)]
        fn will_connect(&self, scene: &AnyObject, _session: &AnyObject, _options: &AnyObject) {
            connect(scene);
        }
    }
);

unsafe impl NSObjectProtocol for WinitSceneDelegate {}

/// Registers the delegate class (and its `UIWindowSceneDelegate` conformance) so UIKit finds it
/// by name. Call before `UIApplicationMain`.
pub(crate) fn register(_mtm: MainThreadMarker) {
    let cls = WinitSceneDelegate::class();
    let name = CStr::from_bytes_with_nul(b"UIWindowSceneDelegate\0").unwrap();
    unsafe {
        let protocol = objc2::ffi::objc_getProtocol(name.as_ptr());
        if !protocol.is_null() {
            objc2::ffi::class_addProtocol(cls as *const _ as *mut objc2::ffi::objc_class, protocol);
        }
    }
}

fn connect(scene: &AnyObject) {
    let scene = unsafe { Retained::retain(scene as *const AnyObject as *mut AnyObject) }
        .expect("a connected scene");
    SCENE.with(|s| *s.borrow_mut() = Some(scene.clone()));
    let waiting = WAITING.with(|w| std::mem::take(&mut *w.borrow_mut()));
    for window in waiting {
        let _: () = unsafe { msg_send![&window, setWindowScene: &*scene] };
        window.makeKeyAndVisible();
    }
}

/// Puts a new window into the connected scene, or keeps it until the scene connects.
pub(crate) fn adopt(window: &Retained<WinitUIWindow>) {
    let scene = SCENE.with(|s| s.borrow().clone());
    match scene {
        Some(scene) => {
            let _: () = unsafe { msg_send![window, setWindowScene: &*scene] };
        },
        None => WAITING.with(|w| w.borrow_mut().push(window.clone())),
    }
}
