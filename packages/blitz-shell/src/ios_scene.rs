//! Single-scene UIKit hosting for the default Blitz event loop.
//!
//! The application's Info.plist must select BlitzSceneDelegate and disable
//! multiple scenes. UIKit owns the scene; Rust keeps only weak window handles.

use std::cell::RefCell;
use std::sync::{Arc, Weak};

use objc2::rc::Retained;
use objc2::{ClassType, define_class};
use objc2_foundation::{NSObject, NSObjectProtocol};
use objc2_ui_kit::{
    UIResponder, UIScene, UISceneConnectionOptions, UISceneDelegate, UISceneSession, UIView,
    UIWindowScene, UIWindowSceneDelegate,
};
use winit::raw_window_handle::{HasWindowHandle, RawWindowHandle};
use winit::window::Window;

thread_local! {
    static SCENE: RefCell<Option<Retained<UIWindowScene>>> = const { RefCell::new(None) };
    static WINDOWS: RefCell<Vec<Weak<dyn Window>>> = const { RefCell::new(Vec::new()) };
}

define_class!(
    #[unsafe(super(UIResponder, NSObject))]
    #[name = "BlitzSceneDelegate"]
    struct BlitzSceneDelegate;

    unsafe impl NSObjectProtocol for BlitzSceneDelegate {}

    unsafe impl UISceneDelegate for BlitzSceneDelegate {
        #[unsafe(method(scene:willConnectToSession:options:))]
        fn connect(
            &self,
            scene: &UIScene,
            _session: &UISceneSession,
            _options: &UISceneConnectionOptions,
        ) {
            let Some(scene) = scene.downcast_ref::<UIWindowScene>() else {
                return;
            };
            // SAFETY: UIKit supplies a live scene for the duration of this
            // callback. Retain it until the matching disconnect notification.
            let retained = unsafe { Retained::retain(scene as *const _ as *mut UIWindowScene) }
                .expect("UIKit supplied a null scene");
            SCENE.with(|current| *current.borrow_mut() = Some(retained));
            for window in live_windows() {
                attach(&*window, scene);
            }
        }

        #[unsafe(method(sceneDidBecomeActive:))]
        fn active(&self, _scene: &UIScene) {
            for window in live_windows() {
                window.request_redraw();
            }
        }

        #[unsafe(method(sceneDidDisconnect:))]
        fn disconnect(&self, scene: &UIScene) {
            SCENE.with(|current| {
                let matches = current.borrow().as_ref().is_some_and(|current| {
                    let current_scene: &UIScene = current;
                    std::ptr::eq(current_scene, scene)
                });
                if matches {
                    *current.borrow_mut() = None;
                }
            });
        }
    }

    unsafe impl UIWindowSceneDelegate for BlitzSceneDelegate {}
);

pub(crate) fn register() {
    // Register before UIApplicationMain resolves the class named by Info.plist.
    let _ = BlitzSceneDelegate::class();
}

fn live_windows() -> Vec<Arc<dyn Window>> {
    WINDOWS.with(|windows| {
        let mut windows = windows.borrow_mut();
        windows.retain(|window| window.strong_count() > 0);
        windows.iter().filter_map(Weak::upgrade).collect()
    })
}

pub(crate) fn prepare_window(window: &Arc<dyn Window>) {
    // Winit's UIKit backend currently makes the window visible in its
    // constructor even when with_visible(false) was requested. Keep it hidden
    // until Blitz has installed accessibility and applied requested visibility.
    if let Some(native) = native_window(&**window) {
        native.setHidden(true);
    }
    WINDOWS.with(|windows| windows.borrow_mut().push(Arc::downgrade(window)));
    let scene = SCENE.with(|current| current.borrow().clone());
    if let Some(scene) = scene {
        attach(&**window, &scene);
    }
}

fn native_window(window: &dyn Window) -> Option<Retained<objc2_ui_kit::UIWindow>> {
    let RawWindowHandle::UiKit(handle) = window.window_handle().ok()?.as_raw() else {
        return None;
    };
    // SAFETY: Winit owns this live UIView. All callers run on UIKit's main thread.
    let view = unsafe { &*handle.ui_view.as_ptr().cast::<UIView>() };
    view.window()
}

fn attach(window: &dyn Window, scene: &UIWindowScene) {
    if let Some(native) = native_window(window) {
        let visible = !native.isHidden();
        native.setWindowScene(Some(scene));
        if visible {
            native.makeKeyAndVisible();
        }
        window.request_redraw();
    }
}
