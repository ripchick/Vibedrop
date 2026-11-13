# Changelog

## Unreleased

## 0.13.0

### Added
- **Music section**: import a Spotify track or album by link and play it through the same
  headphones + microphone routes as the soundboard.
  * Spotify metadata without a user login: the public `open.spotify.com/embed/...` page by
    default, or the Spotify Web API (Client Credentials) when a Client ID/Secret is configured
    in Settings → Music / `SPOTIFY_CLIENT_ID` + `SPOTIFY_CLIENT_SECRET`.
  * Cascading download with `yt-dlp`: SoundCloud `scsearch1:` first (accepted only when the
    duration is within 15 s of Spotify's), YouTube `ytsearch1:` with "official audio" as the
    fallback (20 s tolerance); audio is converted to MP3 by `ffmpeg` in a separate cache.
  * Per-track states (Queued / Downloading / Ready / Error with a retry), album grouping,
    album queue, auto-advance, pause/resume/seek, and Repeat Off / All / One.
  * Music up to 15 minutes per track (`MAX_MUSIC_SECS`), separate `music.json` + `music/`
    cache, "Clear cache", and a tools check with install hints in Settings → Music.
  * New guide: [`docs/MUSIC.md`](docs/MUSIC.md).
- New application icon: a green five-bar equalizer on a dark rounded tile
  (the old placeholder read as random noise). Linux packages refresh the
  icon cache after install (`gtk-update-icon-cache`), so the new icon shows
  up immediately.
- User-facing texts are now OS-aware instead of assuming Windows: device-list
  errors, the "All devices ..." header, the microphone-route hint and the
  protocol registration message use system/PipeWire/PulseAudio wording on
  Linux; Windows-only wording is kept for Windows builds.
- Linux packages now set up the virtual microphone cable themselves (the
  Soundpad-style virtual mic, built only on what the audio server provides,
  no driver blobs), with the full VB-CABLE device pair:
  * `VibeDrop_Route` — the cable's playback side ("CABLE Input"), a system-wide
    PipeWire null sink created from `pipewire.conf.d` with
    `node.priority.session = -1000` so it can never become the default output;
  * `VibeDrop Microphone (Virtual)` — the cable's recording side ("CABLE
    Output"): a `pw-loopback` helper started by PipeWire itself copies
    everything played into the sink to the capture ports of a source node that
    voice chats select as an ordinary microphone;
  * the ALSA pcm `vibedrop_route` (`/etc/asound.conf`, requires
    `pipewire-alsa`) plays into the sink and, for the in-app route test,
    records from the virtual microphone, so the test covers the whole cable.
  Upgrades migrate the previous manual per-user setup (pactl null-sink
  snippet, `~/.asoundrc`) and restart the user audio stack once.

### Fixed
- Music: a long track is no longer stopped with a misleading "could not be decoded" error. The
  engine decodes the whole file before it starts playing, so the limit for that step now grows
  with the length of the track instead of being a fixed 45 seconds.
- Header transport: the master-volume slider and its percentage no longer run off the right
  edge, and the slider handle is solid black instead of letting the blue track show through.
- Music page: a long album title no longer overlaps the "Play all / Download missing / Remove
  album" buttons; the title and player line are truncated inside their own column.
- Settings: the "Open the official VB-CABLE page" button now actually opens
  <https://vb-audio.com/Cable/> (its click was never handled; links now use the same
  platform opener as the data-folder button).
- Sidebar footer: the sound/favorite counter no longer wraps into the Quit row when a music
  track count was added.
- Linux: the microphone route could silently sit on an ALSA alias of the desktop's default
  output ("pipewire", "default", "sysdefault:CARD=...", ...). Names differ, the physical
  device does not, so the name-based conflict check passed and the mixed-in voice played in
  the user's own headphones instead of into the route. Devices now resolve to physical
  targets: the route refuses the default output everywhere (the dropdown no longer offers
  those aliases), and two devices that address the same `CARD=` are treated as one
  destination. PipeWire null sinks ("auto_null.*", the packaged "vibedrop_route" pcm) are
  classified as virtual cables; `dmix:`/`dsnoop:` plumbing is hidden from the route list;
  the empty-device-list help and the "no cable found" notice no longer give Windows advice
  on Linux; the setup guide has a Linux variant ( pactl null-sink recipe).

### Removed
- The experimental Windows capture-APO architecture ("VibeDrop APO") and with it every component
  that was needed only for shipping a signed system audio package:
  - `native/apo/` (COM APO DLL, component/extension INF, package assembly/signing/registration
    PowerShell scripts, mock-Windows COM lifecycle tests);
  - `native/` C++/CMake core (`vibedrop_core`, ABI headers, `build_apo.cmd`, `check.sh`);
  - `src/ffi/`, `src/audio/apo_ipc.rs` and the `capture_apo*` settings/engine/GUI code paths;
  - `tests/abi_layout_tests.rs`, `tests/abi_native_tests.rs`;
  - `docs/APO_ARCHITECTURE.md`, `docs/APO_INSTALL.md`, `docs/ABI_CONTRACT.md` and the APO
    analysis sections of `docs/VIRTUAL_MIC.md`.
- Rationale: VibeDrop does not develop, build, install or ship an own audio driver/system audio
  component. Sound → voice chat works through existing system facilities: a virtual audio cable
  on Windows (e.g. VB-CABLE) and the audio server's routing facilities on Linux (PipeWire).

### Changed
- Settings written by APO-era builds load cleanly: the extra `capture_apo*` fields are ignored.
- The microphone route UI explains the virtual-cable route only and no longer offers APO modes.

### Added
- Full RU/EN localisation (`src/lang.rs`): a Language selector in settings; all UI strings and
  core error message bodies translated (147 static dictionary entries + 98 error phrase
  substitutions + bilingual dynamic sentences); `tests/lang_tests.rs`. Names of devices/files,
  protocol schemes and OS-embedded error texts stay untranslated by design.

### New
- Packaging preparation: AUR recipe (`packaging/arch/PKGBUILD`), Debian packaging
  (`packaging/debian`), RPM spec (`packaging/rpm`) - see `packaging/README.md` for what is
  tested and what is not.
- GitHub Actions CI (`.github/workflows`): format check, clippy, tests, and release builds for
  Windows and Linux on tag pushes.
- `LICENSE` (MIT), this changelog, and an expanded README with Windows/Linux routing guides.

## 0.1.0 (pre-rearchitecture)
- Initial Rust implementation: sound library with favourites/search/duplicates detection,
  global hotkeys, two simultaneous outputs (main + microphone route), software mixer with
  limiter and soft clipper, WAV/MP3 playback via symphonia, `soundpad://`/`vibedrop://` link
  protocol (user-level registration), sound downloader with SSRF guards, green/black theme.
