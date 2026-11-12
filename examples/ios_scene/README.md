# UIKit scene example

This example renders a small Blitz document inside a single UIKit scene. On iOS 27, apps built with the latest SDK must adopt UIKit's scene lifecycle to launch. The example's [Info.plist](Info.plist) selects `BlitzSceneDelegate`, which the default Blitz event loop registers before UIKit starts. The delegate attaches Blitz windows to the scene and requests a redraw when it becomes active. [Apple's UIKit migration guide](https://developer.apple.com/documentation/uikit/transitioning-to-the-uikit-scene-based-life-cycle).

## Run on the iOS Simulator

Use an Apple silicon Mac with Xcode selected and an iOS Simulator runtime installed. Boot an iPhone simulator in Xcode and copy its identifier from `xcrun simctl list devices booted`.

From the root of the Blitz checkout, replace `SIMULATOR_UDID` with that identifier and run:

```sh
rustup target add aarch64-apple-ios-sim
export CARGO_TARGET_DIR="${CARGO_TARGET_DIR:-$PWD/target}"
IPHONEOS_DEPLOYMENT_TARGET=18.4 cargo build --locked -p ios_scene --target aarch64-apple-ios-sim

device=SIMULATOR_UDID
app="$CARGO_TARGET_DIR/ios_scene.app"
mkdir -p "$app"
cp examples/ios_scene/Info.plist "$app/Info.plist"
cp "$CARGO_TARGET_DIR/aarch64-apple-ios-sim/debug/ios_scene" "$app/ios_scene"
codesign --force --sign - "$app"
xcrun simctl install "$device" "$app"
xcrun simctl launch "$device" org.dioxuslabs.blitz.ios-scene
```

Look for **Blitz UIKit Scene**. To check reactivation, open Settings and then return to the app. The same window should render again.

```sh
xcrun simctl launch "$device" com.apple.Preferences
xcrun simctl launch "$device" org.dioxuslabs.blitz.ios-scene
```

The example uses Vello Hybrid because it can render on the iOS Simulator without the indirect GPU execution required by the full Vello renderer.
