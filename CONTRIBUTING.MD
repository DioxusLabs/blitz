# Contributing to Blitz

Welcome to the Dioxus community!
Blitz is a "native" HTML/CSS renderer built to support the "Dioxus Native" project. It is effectively a lightweight webview except that the JavaScript engine is replaced with a native Rust API which allows Rust reactivity / state management libraries like Dioxus to interface with it directly.

Talk to us in: the #native channel in the [Dioxus Discord](https://discord.gg/BWTrn6d3)

## Development

### Windows
Building Blitz requires Python, which can be installed from the Windows app store.

### Linux
Requirements:
 * asound2
 * atk1.0
 * gtk-3
 * udev
 * pango1.0
 * xdo

For example on Ubuntu you can install these by running:
```sh
sudo apt-get update
sudo apt-get install \
    libasound2-dev \
    libatk1.0-dev \
    libgtk-3-dev \
    libudev-dev \
    libpango1.0-dev \
    libxdo-dev
```

### VSCode
You can add the following JSON to your `.vscode/settings.json` to automically build Blitz on all supported targets.
```json
{
    "rust-analyzer.check.features": "all",
    "rust-analyzer.cargo.features": "all",
    "rust-analyzer.check.allTargets": true,
    "rust-analyzer.cargo.allTargets": true
}
```
