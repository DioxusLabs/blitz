#!/bin/bash
# Builds the Blitz widget demo APK without Gradle, using aapt2/javac/d8/apksigner.
# Prerequisites:
#   - ANDROID_HOME with build-tools 35.0.0 and platforms/android-35
#   - Rust cdylibs already built:
#       target/{x86_64,aarch64}-linux-android/release/libblitz_widget_ffi.so
set -euo pipefail

DIR="$(cd "$(dirname "$0")" && pwd)"
REPO="$(cd "$DIR/../.." && pwd)"
BT="$ANDROID_HOME/build-tools/35.0.0"
PLATFORM="$ANDROID_HOME/platforms/android-35/android.jar"
OUT="$DIR/build"

rm -rf "$OUT"
mkdir -p "$OUT/compiled" "$OUT/gen" "$OUT/classes" "$OUT/dex" "$OUT/apk"

# 1. Compile and link resources
"$BT/aapt2" compile --dir "$DIR/res" -o "$OUT/compiled/res.zip"
"$BT/aapt2" link \
    -I "$PLATFORM" \
    --manifest "$DIR/AndroidManifest.xml" \
    --min-sdk-version 26 --target-sdk-version 35 \
    --version-code 1 --version-name 1.0 \
    --java "$OUT/gen" \
    -o "$OUT/apk/base.apk" \
    "$OUT/compiled/res.zip"

# 2. Compile Java and dex
javac --release 11 -classpath "$PLATFORM" -d "$OUT/classes" \
    "$OUT/gen/dev/dioxus/blitzwidget/R.java" \
    "$DIR"/src/dev/dioxus/blitzwidget/*.java
"$BT/d8" --release --lib "$PLATFORM" --min-api 26 \
    --output "$OUT/dex" "$OUT/classes/dev/dioxus/blitzwidget/"*.class

# 3. Assemble APK: dex + native libs
cd "$OUT/apk"
cp "$OUT/dex/classes.dex" .
zip -q base.apk classes.dex
for abi_pair in "x86_64:x86_64-linux-android" "arm64-v8a:aarch64-linux-android"; do
    abi="${abi_pair%%:*}"
    triple="${abi_pair##*:}"
    so="$REPO/target/$triple/release/libblitz_widget_ffi.so"
    if [ -f "$so" ]; then
        mkdir -p "lib/$abi"
        cp "$so" "lib/$abi/"
        zip -q base.apk "lib/$abi/libblitz_widget_ffi.so"
    fi
done

# 4. Align and sign with the debug keystore
KEYSTORE="$HOME/.android/debug.keystore"
if [ ! -f "$KEYSTORE" ]; then
    keytool -genkeypair -keystore "$KEYSTORE" -storepass android -keypass android \
        -alias androiddebugkey -dname "CN=Android Debug,O=Android,C=US" \
        -keyalg RSA -keysize 2048 -validity 10000
fi
"$BT/zipalign" -f 4 base.apk aligned.apk
"$BT/apksigner" sign --ks "$KEYSTORE" --ks-pass pass:android \
    --out "$DIR/blitz-widget-demo.apk" aligned.apk

echo "Built $DIR/blitz-widget-demo.apk"
