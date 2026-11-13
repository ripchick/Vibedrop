//! Music section: Spotify import, cascading download through yt-dlp and album playback.
//!
//! The section is intentionally separate from the soundboard library:
//! * [`library`] stores albums/tracks in `music.json` and the MP3 cache in `music/`;
//! * [`spotify`] turns a track/album link into metadata (Web API or the public page);
//! * [`download`] finds the audio on SoundCloud and falls back to YouTube, checking durations;
//! * [`player`] owns the album queue and the three repeat modes;
//! * [`tools`] locates `yt-dlp`/`ffmpeg` and runs them safely.
//!
//! Playback itself uses the existing audio engine, so music is heard on the same headphones and
//! microphone route as the soundboard, with the same master/route volumes and limiter.

pub mod download;
pub mod library;
pub mod player;
pub mod spotify;
pub mod tools;

pub use library::{MusicAlbum, MusicLibrary, MusicTrack, TrackStatus};
pub use player::{MusicPlayer, PlaybackState, RepeatMode};
pub use spotify::{FetchedEntity, SpotifyKind, SpotifyLink};
pub use tools::Tools;
