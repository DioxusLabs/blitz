// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, Rect, TreeUpdate};
use accesskit_unix::Adapter as UnixAdapter;
use raw_window_handle::RawWindowHandle;

pub struct Adapter {
    adapter: UnixAdapter,
}

impl Adapter {
    pub fn new(
        _window_handle: RawWindowHandle,
        activation_handler: impl 'static + ActivationHandler + Send,
        action_handler: impl 'static + ActionHandler + Send,
        deactivation_handler: impl 'static + DeactivationHandler + Send,
    ) -> Self {
        let adapter = UnixAdapter::new(activation_handler, action_handler, deactivation_handler);
        Self { adapter }
    }

    fn set_root_window_bounds(&mut self, outer: Rect, inner: Rect) {
        self.adapter.set_root_window_bounds(outer, inner);
    }

    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        self.adapter.update_if_active(updater);
    }

    fn update_window_focus_state(&mut self, is_focused: bool) {
        self.adapter.update_window_focus_state(is_focused);
    }

    pub fn set_focus(&mut self, is_focused: bool) {
        self.update_window_focus_state(is_focused);
    }

    pub fn set_window_bounds(&mut self, outer_bounds: Rect, inner_bounds: Rect) {
        self.set_root_window_bounds(outer_bounds, inner_bounds)
    }
}
