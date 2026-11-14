# Changelog

## Unreleased

## 0.14.1

### Added
- Discord Rich Presence shows the cover of the album that is playing. Discord accepts a direct
  picture address in the activity, so no upload to the developer portal is needed; the title of
  the track is the tooltip of the picture.

### Changed
- Smoother animations: the page transition is a 320 ms ease-in-out, the buttons and the sidebar
  glow and fade in and out on hover, the notifications fade in and out, and the bar next to a
  playing sound pulses gently.

## 0.14.0

### Added
- Buttons in Settings -> Music that open the official download pages of yt-dlp and FFmpeg,
  the same way the VB-CABLE button opens the official cable page.

### Fixed
- Discord Rich Presence no longer freezes: the answers of Discord are read away again, so the
  pipe cannot fill up and block the writes.
- The Russian texts that were damaged by a wrong encoding (for example the language selector)
  are proper UTF-8 again.

### Changed
- yt-dlp and FFmpeg are **not** part of the release archives any more. They are separate
  programs with their own licenses; VibeDrop only opens their official download pages.

## 0.13.2

### Added
- **Spotify playlists** in the Music section: a `open.spotify.com/playlist/...` link is imported as
  one collection with its cover, title, owner, tracks, queue and repeat modes. Storage keeps the
  collection kind, so playlists are labelled as playlists after a restart. The Web API path reads
  every page (up to 50 pages); the public page path works without keys. Local files and podcast
  episodes inside a playlist are skipped.

### Changed
- Settings: every numeric setting now uses the same hand-painted 100 px slider as the volume
  controls: corner radius, font size, start offset, simultaneous sounds and a sound's own volume
  in its "•••" menu. Their labels are localised too (the old ones always said "Corner radius",
  "Font size", ... in English).
- Settings: the volume sliders in the "Volume and playback" card now use the same 100 px painted
  rail and percentage as the master slider in the header (they were 180 px `egui::Slider`s).
- Settings: the accent colour field shows its `#RRGGBB` value by default and clearing it restores
  the colour that is currently in use, so the value can no longer be erased.
- Music: an album or playlist is selected by clicking anywhere on its card (cover, title, meta or
  empty space); the play button stays a separate action. Card titles/metadata no longer swallow
  that click as selectable text.
- Music: album rows now have the same 38 px height, 30 px cover and 30 px play button as the track
  rows, and the play button reserves its own space, so a long title is truncated instead of being
  drawn underneath the button.

### Removed
- The "Restore to My Sounds" button and all the code behind it (`restore_sound`,
  `find_sound_by_name`, the `path`/`url` fields of the history entry). A removed sound no longer
  offers to come back from the History page.

## 0.13.1

### Added
- **Real album covers** in the Music section: the cover of the album is fetched once from the
  address Spotify returned with the metadata, kept in the data folder and shown in the player,
  next to the album title and in the album list.
- **"Restore to My sounds"** on the History page: a sound that was removed from the library can
  be put back — from the file on disk, from the address it was downloaded from, or from a file
  with the same name in VibeDrop's own sound folder.
- **"Clear data"** in Settings -> Data folder: removes the library, the history, the music
  albums and the cached MP3s (the settings are kept). A confirmation window asks first, and the
  word `DELETE` has to be typed so that a stray click cannot wipe anything.
- **yt-dlp and ffmpeg ship with the release.** The archives now contain a `tools/` folder with
  both programs, so the Music section works right after the download. They stay separate
  programs with their own licenses; see `THIRD_PARTY_NOTICES.md` and `tools/licenses/`.

### Fixed
- The volume slider in the header no longer reaches over the percentage next to it: the slider
  is painted by hand and its handle stays inside the widget.
- The accent colour field accepts typing again. An incomplete colour was wiped on every save
  (the settings only keep complete colours), so nothing could be typed; the text being edited
  now lives on its own and is applied as soon as it is a valid colour.

### Removed
- The "Check files" button and the code behind it. Sounds whose file disappeared are still
  marked in the library and can be relinked with "Locate file...".

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
