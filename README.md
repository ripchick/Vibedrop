# VibeDrop

![CI](https://github.com/ripchick/Vibedrop/actions/workflows/ci.yml/badge.svg)
[![License: Apache-2.0](https://img.shields.io/badge/License-Apache--2.0-blue.svg)](LICENSE)
[![Rust](https://img.shields.io/badge/Rust-1.80%2B-orange.svg)](https://www.rust-lang.org/)
[![Platform](https://img.shields.io/badge/Platform-Windows%20%7C%20Linux%20%7C%20macOS-lightgrey.svg)](#)

Free, open-source soundboard for voice chats, in the spirit of Soundpad: a sound library with
global hotkeys, downloads via `soundpad://` links and a dual output — your headphones hear the
sound, and the voice chat hears it through a second "microphone route". A desktop-first
Windows-style interface (Fluent look, Segoe UI, Soundpad-style layout — sidebar, sound
table and player bar) — no audio driver bundled or required, Windows, Linux and macOS
support.

[Русский перевод](README.ru.md)

## Screenshots

| Library | Favorites |
| --- | --- |
| ![Library](docs/screenshots/library.png) | ![Favorites](docs/screenshots/favorites.png) |

| History | Settings |
| --- | --- |
| ![History](docs/screenshots/history.png) | ![Settings](docs/screenshots/settings.png) |

* License: Apache-2.0 (see [`LICENSE`](LICENSE))
* Status & history: [`PROGRESS.md`](PROGRESS.md), [`CHANGELOG.md`](CHANGELOG.md)
* Audio routing guide: [`docs/AUDIO_ROUTING.md`](docs/AUDIO_ROUTING.md)

## How it works

* **Local playback** always works out of the box — no virtual device is needed. Sounds are
  decoded once, previewed before use, and mixed with a software mixer (limiter + soft clipper,
  independent volume per sound/route, instant stop, retrigger policies).
* **Voice-chat routing** uses your operating system's own facilities:
  * Windows: a virtual audio cable (e.g. free [VB-CABLE](https://vb-audio.com/Cable/)). VibeDrop
    plays into `CABLE Input`; Discord picks `CABLE Output` as its microphone. VibeDrop detects
    installed cables automatically, shows the paired recording side to use, runs a route test
    and warns if no cable is installed. It never downloads or runs installers for you.
  * Linux: PipeWire/PulseAudio virtual sources (see [`docs/AUDIO_ROUTING.md`](docs/AUDIO_ROUTING.md)).
  * VibeDrop ships **no audio driver and never modifies system audio components**. An older
    experimental capture-APO route was evaluated and removed in this release
    (see `CHANGELOG.md`).
* **Your microphone** (optional): enable "Mix my microphone into the microphone route" to send
  your voice and the soundboard over the one device Discord listens to. The physical mic is
  only opened on request, never opened as an output, and the route's own recording side is
  refused as a voice source (feedback-loop protection).

## Features

- Sound library: add/remove/rename/search/sort sounds, favourites, duplicates detection,
  durations; errors for missing/damaged files stay visible, and the library survives restarts.
- Supported audio formats: **WAV** and **MP3** (via `symphonia`), the same formats the decoder
  uses for playback and preview.
- Global hotkeys: per-sound assignment, re-binding/removal, conflict detection, an explicit
  mic-mute hotkey, messages when a combination cannot be registered, released on exit
  (`global-hotkey` crate; on Linux registration depends on the desktop environment — see
  known limitations).
- Play/stop/volume controls, overlap protection, queue-free "tickets" so Stop always beats
  Play, damaged/audio-file handling that reports instead of panicking.
- Sound downloads from approved `soundpad://`/`vibedrop://` links: scheme-validated parsing,
  rejects private IPs and non-HTTP(S) targets, size limits, no auto-download without user
  action, pause/cancel, part-file cleanup, files never executed.
- Interface in **Russian or English** (switch in Settings → Application; the whole UI is one
  language at a time, including error messages from the core modules).
- Settings organized into cards, dark Fluent look (Segoe UI, Windows-style) with a sidebar,
  sound table and player bar, toasts instead of panics, keyboard-friendly focus handling.
- Discover page: legal free-music search (Creative Commons / public domain, Internet
  Archive) with add-to-library and artist subscriptions.
- Virtual output helper: download the free VB-CABLE (Windows) from the official site,
  or create a built-in virtual sink on Linux — no driver ships inside VibeDrop.

## Windows: installation

1. Download the release zip (or build from source below) and unpack it anywhere.
2. Run `vibedrop.exe` (per-user settings live in `%LOCALAPPDATA%\VibeDrop\VibeDrop\data`).
3. Optional: install VB-CABLE yourself if you want voice-chat routing, then follow the
   on-screen "How to set up the microphone route" help or
   [`docs/AUDIO_ROUTING.md`](docs/AUDIO_ROUTING.md).
   Free official download: <https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip>
   (or via Settings → VIRTUAL OUTPUT (FREE) → "Download free VB-CABLE", no driver ships
   inside VibeDrop itself).

## Linux: installation

Build from source (see below). Distro packages are prepared but not yet published:
- AUR recipe: [`packaging/arch/PKGBUILD`](packaging/arch/PKGBUILD) (needs a maintainer + real
  checksums after the first tag; see [`packaging/README.md`](packaging/README.md))
- Debian packaging: [`packaging/debian`](packaging/debian) (prepared, never built)
- RPM spec: [`packaging/rpm/vibedrop.spec`](packaging/rpm/vibedrop.spec) (prepared, never built)

Runtime libraries the Linux build needs: ALSA + desktop GUI libraries
(`libasound2`, `libxkbcommon`, `libxkbcommon-x11` on X11, `wayland` for Wayland sessions;
`Ubuntu`: `libasound2-dev libxkbcommon-dev libxkbcommon-x11-dev libx11-dev libwayland-dev`).

## macOS: installation

Download the release tarball (`vibedrop-macos-arm64.tar.gz`, Apple Silicon) and unpack it.
The binary is **not signed/notarized**, so macOS may block the first launch: right-click
`vibedrop` in Finder → **Open**, or run:

```sh
xattr -d com.apple.quarantine vibedrop
```

Then run `./vibedrop`. Global hotkeys on macOS need Accessibility permission for the app
(System Settings → Privacy & Security → Accessibility) — without it the app simply
reports that hotkeys are unavailable. Data lives in `~/Library/Application Support/`.
Soundpad-style `soundpad://` link registration is not wired up on macOS yet.

## Building from source

Requirements: Rust 1.80+ (edition 2021, stable toolchain), plus the Linux system packages
listed above on Linux.

```sh
rustup component add rustfmt clippy   # once
cargo fmt --all
cargo test --all-targets
cargo build --release
# Windows: target\release\vibedrop.exe   Linux: target/release/vibedrop
```

CI (`.github/workflows/ci.yml`) runs the same checks on Windows and Ubuntu: `cargo fmt
--check`, `cargo test`, `cargo clippy -D warnings`. No CI step needs a real audio device: the
audio tests are written to pass with no playback/recording hardware present.

## Known limitations

- VibeDrop cannot magically send program audio into the microphone; a virtual device (Windows
  cable, Linux PipeWire loopback) is required for voice-chat routing. Without it VibeDrop
  says so plainly and playback still works.
- Linux GUI/runtime behaviour of VibeDrop itself has **not been verified on the target OS**
  yet (development happens on Windows) — reproducible reports and PRs are very welcome.
- Global hotkeys on Linux depend on the desktop environment (global-hotkey uses the same API
  Valve's games use); some Wayland compositors may not allow them — the app reports
  "Global hotkeys are unavailable" in that case rather than pretending.
- `soundpad://` compatibility covers the link subset validated by tests; third-party sites
  using exotic link shapes are not guaranteed to work, and unsupported actions are rejected.
- The "microphone route" test tone is audible to people in your voice chat (that is how
  it verifies the route); run it with nobody listening.

## Report an issue

Provide your OS, audio setup (which cable), the exact wording of the message you saw, and the
steps to reproduce. Bug templates: `.github/ISSUE_TEMPLATE` (fall back to plain text).

## Acknowledgements

VibeDrop is an independent open-source project (Apache-2.0 license) — it is **not** the
original Soundpad program. Soundpad is a trademark of its respective owner; this project is
not affiliated with or endorsed by that owner. VB-CABLE is a freeware product of VB-Audio;
it is not included in this repository and VB-Audio is not affiliated with this project.
All other trademarks (Discord, PipeWire, PulseAudio, Linux, Windows) belong to their
respective owners.
