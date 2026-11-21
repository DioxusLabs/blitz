// Copyright 2025 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

use accesskit::{ActionHandler, ActivationHandler, DeactivationHandler, Rect, TreeUpdate};
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
        // AndroidApp can be used from a native thread which is not attached yet.
        let mut env = vm.attach_current_thread().unwrap();
        let activity = unsafe { JObject::from_raw(android_app.activity_as_ptr() as *mut _) };
        let adapter = env
            .with_local_frame(
                16,
                |env| -> Result<_, accesskit_android::jni::errors::Error> {
                    // Public Activity APIs work with both NativeActivity and GameActivity.
                    // A private GameActivity field is absent on NativeActivity.
                    let window = env
                        .call_method(&activity, "getWindow", "()Landroid/view/Window;", &[])?
                        .l()?;
                    let view = env
                        .call_method(&window, "getDecorView", "()Landroid/view/View;", &[])?
                        .l()?;
                    Ok(InjectingAdapter::new(
                        env,
                        &view,
                        activation_handler,
                        action_handler,
                    ))
                },
            )
            .unwrap();
        Self { adapter }
    }

    pub fn update_if_active(&mut self, updater: impl FnOnce() -> TreeUpdate) {
        self.adapter.update_if_active(updater);
    }

    pub fn set_focus(&mut self, _is_focused: bool) {}

    pub fn set_window_bounds(&mut self, _outer_bounds: Rect, _inner_bounds: Rect) {}
}
