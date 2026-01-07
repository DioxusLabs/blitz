// Copyright 2022 The AccessKit Authors. All rights reserved.
// Licensed under the Apache License, Version 2.0 (found in
// the LICENSE-APACHE file).

// Based loosely on winit's src/platform_impl/mod.rs.

pub use self::platform::*;

#[cfg(target_os = "windows")]
#[path = "windows.rs"]
mod platform;

#[cfg(target_os = "macos")]
#[path = "macos.rs"]
mod platform;

#[cfg(all(
    feature = "accesskit_unix",
    any(
        target_os = "linux",
        target_os = "dragonfly",
        target_os = "freebsd",
        target_os = "netbsd",
        target_os = "openbsd"
    )
))]
#[path = "unix.rs"]
mod platform;

#[cfg(all(feature = "accesskit_android", target_os = "android"))]
#[path = "android.rs"]
mod platform;

#[cfg(not(any(
    target_os = "windows",
    target_os = "macos",
    all(
        feature = "accesskit_unix",
        any(
            target_os = "linux",
            target_os = "dragonfly",
            target_os = "freebsd",
            target_os = "netbsd",
            target_os = "openbsd"
        )
    ),
    all(feature = "accesskit_android", target_os = "android")
)))]
#[path = "null.rs"]
mod platform;
