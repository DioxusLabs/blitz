---
name: testing-ios-widgets
description: How to build, install, and test Blitz-rendered iOS apps/widgets on the iOS Simulator on a macOS box, including WidgetKit gotchas and recording tips.
---

# Testing Blitz on the iOS Simulator (apps and WidgetKit widgets)

## Environment
- `source ~/repos/blitz/.devin-env.sh` for cargo/SDK paths. Xcode must be selected (`xcode-select -p` → /Applications/Xcode.app).
- Boot a simulator: `xcrun simctl boot "iPhone 17" && open -a Simulator`.

## Building
- Rust static/dylib for sim: `cargo build --release -p <crate> --target aarch64-apple-ios-sim`.
- Widget example: `cd examples/ios-widget && xcodebuild -project BlitzWidgetDemo.xcodeproj -scheme BlitzWidgetDemo -destination 'platform=iOS Simulator,name=iPhone 17' CODE_SIGNING_ALLOWED=NO build`, then `xcrun simctl install booted <DerivedData .app path>`.

## Driving the Simulator UI (computer tool)
- Home: Cmd+Shift+H. Lock: Cmd+L. Screenshots: `xcrun simctl io booted screenshot f.png`.
- Long-press = mouse_move + left_mouse_down + wait 2-3s + left_mouse_up. Scroll wheel does NOT scroll iOS content — use drag gestures.
- Add home widget: long-press home → "Edit" (top-left) → "Add Widget" → search → select size page (swipe carousel) → "Add Widget".
- Lock-screen widget: Cmd+L, long-press lock screen (3s+) → gallery → "+" to create a lock screen → tap "ADD WIDGETS" → pick widget → Done → tap wallpaper to activate. Settings→Wallpaper does NOT exist in simulator builds. SpringBoard's customize UI can lag/freeze for ~30-60s; queued clicks may fire late (and can accidentally tap interactive widgets — the counter may jump). Dismiss stuck sheets by dragging their grabber down.
- Hardware-keyboard typing works into system UI (widget gallery search, Settings). Typing into Blitz-rendered `<input>` in the native app examples did NOT work in testing (no IME/key routing) — verify before relying on it.

## WidgetKit gotchas found in testing
- `context.displaySize` is fractional for systemMedium (e.g. 349.67pt). Any pixel-size round-trip between Swift and the Rust FFI must truncate/round identically on both sides or the buffer-length check fails and the widget shows the fallback ("Blitz render failed").
- Widget re-render after an interactive AppIntent tap takes ~1-2s on the simulator.
- In widgets with per-region Button(intent:) overlays, tapping any area NOT covered by a button opens the containing app (default WidgetKit tap behavior) — useful as an adversarial check that regions are where you expect.
- Diagnostics: `xcrun simctl spawn booted log stream --predicate 'process CONTAINS "BlitzWidgetExt"'` (mostly chronod noise; add NSLog to the extension for real errors).

## Recording
- Clean demo (simulator display only, no desktop): `xcrun simctl io booted recordVideo --codec h264 -f out.mp4`, stop with SIGINT.
- The builtin recording_start tool may fail on this box (its ffmpeg picks AVFoundation device index 1, which doesn't exist → exit 251 / "Invalid device index"). Fallback full-screen capture that works: `ffmpeg -f avfoundation -framerate 20 -capture_cursor 1 -i "0:none" -pix_fmt yuv420p out.mp4`.
