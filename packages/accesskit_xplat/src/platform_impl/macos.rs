// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, Rect, TreeUpdate};
use accesskit_macos::SubclassingAdapter;
use raw_window_handle::RawWindowHandle;

pub struct Adapter {
    adapter: SubclassingAdapter,
}

impl Adapter {
    pub fn new(
        window_handle: RawWindowHandle,
        activation_handler: impl 'static + ActivationHandler,
        action_handler: impl 'static + ActionHandler,
        _deactivation_handler: impl 'static + DeactivationHandler,
    ) -> Self {
        let view = match window_handle {
            RawWindowHandle::AppKit(handle) => handle.ns_view.as_ptr(),
            RawWindowHandle::UiKit(_) => unimplemented!(),
            _ => unreachable!(),
        };

        let adapter = unsafe { SubclassingAdapter::new(view, activation_handler, action_handler) };
        Self { adapter }
    }

    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        if let Some(events) = self.adapter.update_if_active(updater) {
            events.raise();
        }
    }

    pub fn set_focus(&mut self, is_focused: bool) {
        if let Some(events) = self.adapter.update_view_focus_state(is_focused) {
            events.raise();
        }
    }

    pub fn set_window_bounds(&mut self, _outer_bounds: Rect, _inner_bounds: Rect) {}
}
