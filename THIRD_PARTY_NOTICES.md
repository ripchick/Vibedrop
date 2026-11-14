# Third-party notices

VibeDrop is an independent project (Apache-2.0). The Music section talks to two separate
programs that are **not** part of VibeDrop and are **not** shipped with it. They are started as
ordinary child processes, exactly like a text editor starts a compiler. VibeDrop only opens their
official download pages so the user can install them.

## yt-dlp

* Project: https://github.com/yt-dlp/yt-dlp
* Downloads: https://github.com/yt-dlp/yt-dlp/releases
* License: The Unlicense (public domain) — https://github.com/yt-dlp/yt-dlp/blob/master/LICENSE
* Copyright: the yt-dlp contributors

yt-dlp is used to look for an openly available copy of a track and to download it. It is not
modified.

## FFmpeg

* Project: https://ffmpeg.org
* Downloads: https://ffmpeg.org/download.html
* License: depends on the build the user installs (LGPL-2.1+ or GPL-2.0+); the license files
  come with that build
* Copyright: the FFmpeg developers and the authors of the libraries it is built with

FFmpeg converts the downloaded audio into MP3. It is not modified. FFmpeg is a separate
executable that the user installs; VibeDrop is neither linked against it nor derived from it.

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
