// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, Rect, TreeUpdate};
use raw_window_handle::RawWindowHandle;

pub struct Adapter;

impl Adapter {
    pub fn new(
        _window_handle: &RawWindowHandle,
        _activation_handler: impl 'static + ActivationHandler,
        _action_handler: impl 'static + ActionHandler,
        _deactivation_handler: impl 'static + DeactivationHandler,
    ) -> Self {
        Self {}
    }

    pub fn update_if_active(&mut self, _updater: impl FnOnce() -> TreeUpdate) {}

    pub fn set_focus(&mut self, _is_focused: bool) {}

    pub fn set_window_bounds(&mut self, _outer_bounds: Rect, _inner_bounds: Rect) {}
}
