// Copyright 2026 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, Rect, TreeUpdate};
use accesskit_ios::SubclassingAdapter;
use raw_window_handle::RawWindowHandle;

pub struct Adapter {
    adapter: SubclassingAdapter,
}

impl Adapter {
    pub fn new(
        window_handle: RawWindowHandle,
        activation_handler: impl 'static + ActivationHandler,
        action_handler: impl 'static + ActionHandler,
        deactivation_handler: impl 'static + DeactivationHandler,
    ) -> Self {
        let view = match window_handle {
            RawWindowHandle::UiKit(handle) => handle.ui_view.as_ptr(),
            _ => unreachable!("the iOS accessibility adapter requires a UIKit view"),
        };

        // SAFETY: The window handle supplies a live UIView on the main thread.
        // Blitz creates this adapter before making the window visible, and the
        // adapter retains the view and restores its original class on drop.
        let adapter = unsafe {
            SubclassingAdapter::new(
                view,
                activation_handler,
                action_handler,
                deactivation_handler,
            )
        };
        Self { adapter }
    }

    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        if let Some(events) = self.adapter.update_if_active(updater) {
            events.raise();
        }
    }

    // AccessKit's UIKit adapter observes native view and assistive-technology
    // lifecycle notifications itself; it has no separate focus-state API.
    pub fn set_focus(&mut self, _is_focused: bool) {}

    // UIKit converts the tree's physical view coordinates to screen points.
    pub fn set_window_bounds(&mut self, _outer_bounds: Rect, _inner_bounds: Rect) {}
}
