// Copyright 2025 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, TreeUpdate};
use accesskit_android::{
    InjectingAdapter,
    jni::{JavaVM, objects::JObject},
};
use android_activity::AndroidApp;

pub struct Adapter {
    adapter: InjectingAdapter,
}

impl Adapter {
    pub fn new(
        android_app: &AndroidApp,
        activation_handler: impl 'static + ActivationHandler + Send,
        action_handler: impl 'static + ActionHandler + Send,
        _deactivation_handler: impl 'static + DeactivationHandler,
    ) -> Self {
        let vm = unsafe { JavaVM::from_raw(android_app.vm_as_ptr() as *mut _) }.unwrap();
        let mut env = vm.get_env().unwrap();
        let activity = unsafe { JObject::from_raw(android_app.activity_as_ptr() as *mut _) };
        let view = env
            .get_field(
                &activity,
                "mSurfaceView",
                "Lcom/google/androidgamesdk/GameActivity$InputEnabledSurfaceView;",
            )
            .unwrap()
            .l()
            .unwrap();
        let adapter = InjectingAdapter::new(&mut env, &view, activation_handler, action_handler);
        Self { adapter }
    }

    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        self.adapter.update_if_active(updater);
    }

    pub fn set_focus(&mut self, is_focused: bool) {}

    pub fn set_window_bounds(&mut self, outer_bounds: Rect, inner_bounds: Rect) {}
}
