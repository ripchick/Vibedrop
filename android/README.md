# Android (experimental)

VibeDrop targets Android (arm64-v8a) via eframe/winit, cpal (Oboe) and reqwest.
The UI adapts to phones: on narrow screens the category rail becomes a horizontal
top strip.

## Status — what works, what does not

- **Code compiles for the Android target** (proven by `.github/workflows/android.yml`:
  `cargo check --lib --target aarch64-linux-android` with the NDK present).
- **Not included yet — needs a real device + the custom entry point:**
  `eframe::run_native` (0.29) cannot create the Android event loop (`winit`
  requires an `AndroidApp` from `android_main()`), so launching the packaged
  `libvibedrop.so` is not wired up yet. Desktop builds are unaffected.
- Not available on Android by design: global hotkeys, `soundpad://` protocol
  registration, and file-picker import (file dialogs). Playback, the library,
  URL downloads and settings work.

## Build the library

```sh
rustup target add aarch64-linux-android
export NDK_HOME=/path/to/ndk
export CC_aarch64_linux_android="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang"
export CXX_aarch64_linux_android="$NDK_HOME/toolchains/llvm/prebuilt/linux-x86_64/bin/aarch64-linux-android24-clang++"
cargo check --lib --target aarch64-linux-android
```

## Package an APK (after the entry point is wired up)

An APK also needs a small Rust entry point plus the cdylib:

```toml
# Cargo.toml (locally, before packaging)
[lib]
crate-type = ["cdylib", "rlib"]
```

plus a `#[no_mangle] pub fn android_main(app: ndk::AndroidApp)` that builds the winit
event loop with `.with_android_app(app)` and drives `egui-winit` (eframe 0.29's
`run_native` cannot create the Android event loop), then:

```sh
cd android
gradle :app:assembleRelease
# → app/build/outputs/apk/release/app-release.apk (debug-signed)
```

Install with `adb install app-release.apk` or copy the file to the phone.
