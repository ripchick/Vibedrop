# Third-party notices

VibeDrop is an independent project (Apache-2.0). The Music section talks to two separate
programs that are **not** part of VibeDrop and are not linked into it. They are started as
ordinary child processes, exactly like a text editor starts a compiler.

## yt-dlp

* Project: https://github.com/yt-dlp/yt-dlp
* License: The Unlicense (public domain) — https://github.com/yt-dlp/yt-dlp/blob/master/LICENSE
* Copyright: the yt-dlp contributors
* Version shipped with the release: 2026.10.26

yt-dlp is used to look for an openly available copy of a track and to download it. It is not
modified. The license text is included in `tools/licenses/`.

## FFmpeg

* Project: https://ffmpeg.org
* License of the shipped build: **GPL-2.0-or-later** (the Windows build by gyan.dev and the
  static Linux build include components that are GPL, for example libmp3lame)
* Copyright: the FFmpeg developers and the authors of the libraries it is built with
* The exact license files of the shipped build are included in `tools/licenses/`

FFmpeg converts the downloaded audio into MP3. It is not modified. FFmpeg is a separate
executable that is distributed next to VibeDrop; VibeDrop itself is neither linked against it
nor derived from it. The complete corresponding source code of the shipped build is available
from the projects named above (the exact build URLs are listed in `tools/licenses/BUILD.txt`).

## Other projects

* **VB-CABLE** — a free virtual audio cable by VB-Audio (https://vb-audio.com/Cable/). It is
  *not* part of VibeDrop; the program only opens the official page so the user can download it.
* **Discord** — the Rich Presence integration talks to the local Discord client over its own
  IPC. Discord is a trademark of its respective owner.
* **Spotify** — the Music section reads the public metadata of a track or album. Spotify is a
  trademark of Spotify AB. No Spotify account is needed and nothing is bypassed.
* **Soundpad** — VibeDrop is not the original Soundpad program. Soundpad is a trademark of its
  respective owner; this project is not affiliated with or endorsed by that owner.
* All other trademarks (PipeWire, PulseAudio, Linux, Windows, macOS) belong to their respective
  owners.
