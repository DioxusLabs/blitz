# accesskit_xplat

[![Crates.io](https://img.shields.io/crates/v/accesskit_xplat.svg)](https://crates.io/crates/accesskit_xplat)
[![Docs](https://docs.rs/accesskit_xplat/badge.svg)](https://docs.rs/accesskit_xplat)
[![Crates.io License](https://img.shields.io/crates/l/accesskit_xplat)](#license)

Cross-platform [AccessKit](github.com/AccessKit/accesskit) adapter similar to [accesskit_winit](https://docs.rs/accesskit_winit), but without depending on Winit.
This allows this crate to be used with any version of Winit without needing a version of crate that matches the version of Winit
that you are using (this is particularly helpful for beta or git versions of Winit that don't usually get an accesskit_winit release).

**WARNING:** The AccessKit developers [have noted](https://github.com/AccessKit/accesskit/pull/656#issuecomment-3706315306) that the approaches used by both this crates and accesskit_winit may not be the
optimal way to implement AccessKit, so use this crate at your own risk. But if you would otherwise be using accesskit_winit then
this crate has no additional caveats (except that it requires little bit of extra boilerplate code.)

## Example usage

Based on Winit `0.31.0-beta.2`'s `Window` and `WindowEvent` types but could be adapted for other versions of Winit.

```rust
use accesskit::Rect;
use accesskit_xplat::{Adapter, EventHandler, WindowEvent as AccessKitEvent};
use raw_window_handle::HasWindowHandle;
use std::sync::Arc;
use winit_core::{
    event::WindowEvent,
    window::{Window, WindowId},
};

/// State of the accessibility node tree and platform adapter.
pub struct AccessibilityState {
    adapter: Adapter,
}

struct Handler {
    window_id: WindowId,
    // Whatever else you like here. Perhaps EventLoopProxy and/or a channel sender.
}
impl EventHandler for Handler {
    fn handle_accesskit_event(&self, event: AccessKitEvent) {
        // Your own custom event handling code
    }
}

impl AccessibilityState {
    pub fn new(window: &dyn Window) -> Self {
        let window_id = window.id();
        Self {
            adapter: Adapter::with_combined_handler(
                // On Android, pass `&android_activity::AndroidApp` when creating the `Adapter`
                #[cfg(target_os = "android")]
                &crate::current_android_app(),
                // On all other platforms, pass `RawWindowHandle` when creating the `Adapter`
                #[cfg(not(target_os = "android"))]
                window.window_handle().unwrap().as_raw(),
                Arc::new(Handler { window_id }),
            ),
        }
    }

    /// Allows reacting to window events.
    ///
    /// This must be called whenever a new window event is received
    /// and before it is handled by the application.
    pub fn process_window_event(&mut self, window: &dyn Window, event: &WindowEvent) {
        match event {
            WindowEvent::Focused(is_focused) => {
                self.adapter.set_focus(*is_focused);
            }
            WindowEvent::Moved(_) | WindowEvent::SurfaceResized(_) => {
                let outer_position: (_, _) = window
                    .outer_position()
                    .unwrap_or_default()
                    .cast::<f64>()
                    .into();
                let outer_size: (_, _) = window.outer_size().cast::<f64>().into();
                let inner_position: (_, _) = window.surface_position().cast::<f64>().into();
                let inner_size: (_, _) = window.surface_size().cast::<f64>().into();

                self.adapter.set_window_bounds(
                    Rect::from_origin_size(outer_position, outer_size),
                    Rect::from_origin_size(inner_position, inner_size),
                )
            }
            _ => (),
        }
    }
}
```

## Compatibility with async runtimes

The following only applies on Linux/Unix:

While this crate's API is purely blocking, it internally spawns asynchronous tasks on an executor.

- If you use tokio, make sure to enable the `tokio` feature of this crate.
- If you use another async runtime or if you don't use one at all, the default feature will suit your needs.

## License

This project is licensed under the Apache 2.0 license.