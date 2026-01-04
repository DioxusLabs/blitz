// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

/// ## Compatibility with async runtimes
///
/// The following only applies on Linux/Unix:
///
/// While this crate's API is purely blocking, it internally spawns asynchronous tasks on an executor.
///
/// - If you use tokio, make sure to enable the `tokio` feature of this crate.
/// - If you use another async runtime or if you don't use one at all, the default feature will suit your needs.

#[cfg(all(
    feature = "accesskit_unix",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ),
    not(feature = "async-io"),
    not(feature = "tokio")
))]
compile_error!("Either \"async-io\" (default) or \"tokio\" feature must be enabled.");

#[cfg(all(
    feature = "accesskit_unix",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    ),
    feature = "async-io",
    feature = "tokio"
))]
compile_error!(
    "Both \"async-io\" (default) and \"tokio\" features cannot be enabled at the same time."
);

use std::sync::Arc;

use accesskit::{
    ActionHandler, ActionRequest, ActivationHandler, DeactivationHandler, Rect, TreeUpdate,
};

#[cfg(target_os = "android")]
use android_activity::AndroidApp;
#[cfg(not(target_os = "android"))]
use raw_window_handle::RawWindowHandle;

mod platform_impl;

#[derive(Clone, Debug, PartialEq)]
pub enum WindowEvent {
    InitialTreeRequested,
    ActionRequested(ActionRequest),
    AccessibilityDeactivated,
}

pub trait EventHandler: Send + Sync + 'static {
    fn handle_accesskit_event(&self, event: WindowEvent);
}

#[derive(Clone)]
struct CombinedHandler(Arc<dyn EventHandler>);

impl ActivationHandler for CombinedHandler {
    fn request_initial_tree(&mut self) -> Option<TreeUpdate> {
        self.0
            .handle_accesskit_event(WindowEvent::InitialTreeRequested);
        None
    }
}
impl DeactivationHandler for CombinedHandler {
    fn deactivate_accessibility(&mut self) {
        self.0
            .handle_accesskit_event(WindowEvent::AccessibilityDeactivated);
    }
}
impl ActionHandler for CombinedHandler {
    fn do_action(&mut self, request: ActionRequest) {
        self.0
            .handle_accesskit_event(WindowEvent::ActionRequested(request));
    }
}

pub struct Adapter {
    /// A user-supplied ID that we pass back to
    inner: platform_impl::Adapter,
}

impl Adapter {
    /// Creates a new AccessKit adapter for a winit window. This must be done
    /// before the window is shown for the first time. This means that you must
    /// use [`winit::window::WindowAttributes::with_visible`] to make the window
    /// initially invisible, then create the adapter, then show the window.
    ///
    /// Use this if you want to provide your own AccessKit handler callbacks
    /// rather than dispatching requests through the winit event loop. This is
    /// especially useful for the activation handler, because depending on
    /// your application's architecture, implementing the handler directly may
    /// allow you to return an initial tree synchronously, rather than requiring
    /// some platform adapters to use a placeholder tree until you send
    /// the first update. However, remember that each of these handlers may be
    /// called on any thread, depending on the underlying platform adapter.
    ///
    /// # Panics
    ///
    /// Panics if the window is already visible.
    pub fn with_split_handlers(
        #[cfg(target_os = "android")] android_app: &AndroidApp,
        #[cfg(not(target_os = "android"))] window: RawWindowHandle,
        activation_handler: impl 'static + ActivationHandler + Send,
        action_handler: impl 'static + ActionHandler + Send,
        deactivation_handler: impl 'static + DeactivationHandler + Send,
    ) -> Self {
        let inner = platform_impl::Adapter::new(
            #[cfg(target_os = "android")]
            android_app,
            #[cfg(not(target_os = "android"))]
            &window,
            activation_handler,
            action_handler,
            deactivation_handler,
        );
        Self { inner }
    }

    pub fn with_combined_handler(
        #[cfg(target_os = "android")] android_app: &AndroidApp,
        #[cfg(not(target_os = "android"))] window: RawWindowHandle,
        handler: Arc<dyn EventHandler>,
    ) -> Self {
        let handler = CombinedHandler(handler);
        let inner = platform_impl::Adapter::new(
            #[cfg(target_os = "android")]
            android_app,
            #[cfg(not(target_os = "android"))]
            &window,
            handler.clone(),
            handler.clone(),
            handler,
        );
        Self { inner }
    }

    /// If and only if the tree has been initialized, call the provided function
    /// and apply the resulting update. Note: If the caller's implementation of
    /// [`ActivationHandler::request_initial_tree`] initially returned `None`,
    /// or if the caller created the adapter using [`EventLoopProxy`], then
    /// the [`TreeUpdate`] returned by the provided function must contain
    /// a full tree.
    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        self.inner.update_if_active(updater);
    }

    pub fn set_focus(&mut self, is_focused: bool) {
        self.inner.set_focus(is_focused);
    }

    pub fn set_window_bounds(&mut self, outer_bounds: Rect, inner_bounds: Rect) {
        self.inner.set_window_bounds(outer_bounds, inner_bounds);
    }
}
