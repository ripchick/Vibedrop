//! Playback queue for the Music section: one track at a time, three repeat modes and an
//! explicit "waiting for the download" state.
//!
//! The player is deliberately independent from the soundboard library: it feeds a unique sound id
//! (`music:<track id>`) to the existing audio engine, so music goes through exactly the same two
//! routes, mixer, limiter and volume controls as every sound. A track that is not downloaded yet
//! parks in [`PlaybackState::Buffering`]; the app asks the download worker for it and the player
//! starts it as soon as the file appears.
//!
//! End-of-track detection uses the engine's playback position: when the voice disappears and the
//! last position was at (or within a second of) the end, the track finished; otherwise playback
//! was stopped from outside and the player pauses instead of skipping.

use crate::audio::engine::{Engine, MAX_MUSIC_SECS};
use crate::music::library::{MusicLibrary, MusicTrack, TrackStatus};
use crate::settings::Settings;
use std::time::{Duration, Instant};

/// Sound ids of music tracks start with this prefix, which lets the app route engine errors.
pub const MUSIC_SOUND_PREFIX: &str = "music:";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    /// Play the album once and stop after the last track.
    #[default]
    Off,
    /// Start the album again after the last track.
    All,
    /// Repeat the current track forever.
    One,
}

impl RepeatMode {
    pub fn next(self) -> RepeatMode {
        match self {
            RepeatMode::Off => RepeatMode::All,
            RepeatMode::All => RepeatMode::One,
            RepeatMode::One => RepeatMode::Off,
        }
    }

    /// English label used by the UI (and by `lang::t` as the dictionary key).
    pub fn label(self) -> &'static str {
        match self {
            RepeatMode::Off => "Repeat: Off",
            RepeatMode::All => "Repeat: All",
            RepeatMode::One => "Repeat: One",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    /// Nothing selected, or playback was stopped.
    Idle,
    /// The engine is decoding / starting the track.
    Loading,
    Playing,
    Paused,
    /// The current track is queued for download; playback starts automatically when it is ready.
    Buffering,
    Error,
}

impl PlaybackState {
    pub fn is_active(self) -> bool {
        matches!(
            self,
            PlaybackState::Loading | PlaybackState::Playing | PlaybackState::Paused | PlaybackState::Buffering
        )
    }
}

/// Base time the engine may take to decode a track before the player reports a failure.
///
/// The engine decodes the whole file before it starts playing, so a long track needs more time
/// than a short one. The limit therefore grows with the length of the track (see
/// [`loading_timeout`]).
const LOADING_TIMEOUT: Duration = Duration::from_secs(45);

/// How long the engine may take to decode a track of `duration_secs`.
///
/// Decoding is roughly proportional to the length of the track, so a fixed limit would kill
/// long tracks (and report a misleading "could not be decoded" error) while they are still
/// being decoded normally.
fn loading_timeout(duration_secs: f64) -> Duration {
    let extra = if duration_secs.is_finite() {
        duration_secs.clamp(0.0, 600.0)
    } else {
        0.0
    };
    LOADING_TIMEOUT + Duration::from_secs_f64(extra)
}

pub struct MusicPlayer {
    album_id: Option<String>,
    queue: Vec<String>,
    index: usize,
    repeat: RepeatMode,
    state: PlaybackState,
    current: Option<String>,
    sound_id: String,
    /// Position to move to once the engine has decoded the track (resume after pause).
    pending_seek_secs: Option<f64>,
    /// Position to resume from when the user presses play after a pause.
    resume_secs: Option<f64>,
    last_pos: f64,
    last_total: f64,
    loading_since: Option<Instant>,
    error: Option<String>,
}

impl Default for MusicPlayer {
    fn default() -> Self {
        MusicPlayer {
            album_id: None,
            queue: Vec::new(),
            index: 0,
            repeat: RepeatMode::Off,
            state: PlaybackState::Idle,
            current: None,
            sound_id: String::new(),
            pending_seek_secs: None,
            resume_secs: None,
            last_pos: 0.0,
            last_total: 0.0,
            loading_since: None,
            error: None,
        }
    }
}

impl MusicPlayer {
    pub fn album_id(&self) -> Option<&str> {
        self.album_id.as_deref()
    }

    pub fn current(&self) -> Option<&str> {
        self.current.as_deref()
    }

    pub fn state(&self) -> PlaybackState {
        self.state
    }

    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    pub fn set_repeat(&mut self, repeat: RepeatMode) {
        self.repeat = repeat;
    }

    pub fn cycle_repeat(&mut self) {
        self.repeat = self.repeat.next();
    }

    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    pub fn queue_len(&self) -> usize {
        self.queue.len()
    }

    pub fn queue_index(&self) -> usize {
        self.index
    }

    pub fn sound_id(&self) -> &str {
        &self.sound_id
    }

    /// `(position, total)` in seconds for the player bar. While paused the last known position is
    /// returned; while buffering the Spotify duration is shown.
    pub fn time(&self) -> (f64, f64) {
        (self.last_pos, self.last_total)
    }

    pub fn current_track<'a>(&self, library: &'a MusicLibrary) -> Option<&'a MusicTrack> {
        let id = self.current.as_deref()?;
        library.track(id)
    }

    /// The track that blocks playback because it still has to be downloaded.
    pub fn waiting_for_download<'a>(&self, library: &'a MusicLibrary) -> Option<&'a MusicTrack> {
        if self.state != PlaybackState::Buffering {
            return None;
        }
        self.current_track(library)
    }

    /// Select an album without starting it (used when the user clicks the album in the list).
    pub fn select_album(&mut self, library: &MusicLibrary, album_id: &str) {
        if self.album_id.as_deref() == Some(album_id) {
            return;
        }
        let Some(album) = library.get(album_id) else { return };
        self.album_id = Some(album_id.to_string());
        self.queue = album.tracks.iter().map(|t| t.id.clone()).collect();
        self.index = 0;
        self.state = PlaybackState::Idle;
        self.current = None;
        self.last_pos = 0.0;
        self.last_total = album.tracks.first().map(|t| t.duration_secs).unwrap_or(0.0);
        self.error = None;
    }

    /// Start an album at `start_index`. If that track is not downloaded, playback waits for it.
    pub fn play_album(
        &mut self,
        library: &MusicLibrary,
        engine: &Engine,
        settings: &Settings,
        album_id: &str,
        start_index: usize,
    ) -> Result<(), String> {
        let Some(album) = library.get(album_id) else {
            return Err("The album is no longer in the music library.".to_string());
        };
        if album.tracks.is_empty() {
            return Err("This album has no tracks.".to_string());
        }
        self.stop_sound(engine);
        self.album_id = Some(album_id.to_string());
        self.queue = album.tracks.iter().map(|t| t.id.clone()).collect();
        self.index = start_index.min(self.queue.len() - 1);
        self.current = None;
        self.pending_seek_secs = None;
        self.resume_secs = None;
        self.last_pos = 0.0;
        self.error = None;
        self.start_current(library, engine, settings)
    }

    /// Start one specific track inside its album (the queue is the whole album).
    pub fn play_track(
        &mut self,
        library: &MusicLibrary,
        engine: &Engine,
        settings: &Settings,
        album_id: &str,
        track_id: &str,
    ) -> Result<(), String> {
        let Some(album) = library.get(album_id) else {
            return Err("The album is no longer in the music library.".to_string());
        };
        let index = album
            .track_index(track_id)
            .ok_or_else(|| "The track is no longer in the music library.".to_string())?;
        self.play_album(library, engine, settings, album_id, index)
    }

    /// Play/pause. Play from Idle (or after an error) replays the selected queue item.
    pub fn toggle(&mut self, library: &MusicLibrary, engine: &Engine, settings: &Settings) -> Result<(), String> {
        match self.state {
            PlaybackState::Playing | PlaybackState::Loading => {
                let pos = engine
                    .progress(&self.sound_id)
                    .map(|(pos, _)| pos)
                    .or(self.resume_secs)
                    .unwrap_or(self.last_pos);
                self.stop_sound(engine);
                self.resume_secs = Some(pos);
                self.state = PlaybackState::Paused;
                self.error = None;
                Ok(())
            }
            PlaybackState::Buffering => {
                self.stop_sound(engine);
                self.resume_secs = Some(self.last_pos);
                self.state = PlaybackState::Paused;
                Ok(())
            }
            PlaybackState::Paused => {
                if self.current.is_none() {
                    self.start_current(library, engine, settings)
                } else {
                    self.pending_seek_secs = self.resume_secs;
                    self.start_current(library, engine, settings)
                }
            }
            PlaybackState::Idle | PlaybackState::Error => {
                self.resume_secs = None;
                self.pending_seek_secs = None;
                self.start_current(library, engine, settings)
            }
        }
    }

    /// Stop music playback and release the engine voice. The queue stays selected.
    pub fn stop(&mut self, engine: &Engine) {
        self.stop_sound(engine);
        self.state = PlaybackState::Idle;
        self.current = None;
        self.resume_secs = None;
        self.pending_seek_secs = None;
        self.last_pos = 0.0;
    }

    /// Forget the queue too (for example when the album is removed).
    pub fn clear(&mut self, engine: &Engine) {
        self.stop(engine);
        self.album_id = None;
        self.queue.clear();
        self.index = 0;
        self.last_total = 0.0;
        self.error = None;
    }

    pub fn next(&mut self, library: &MusicLibrary, engine: &Engine, settings: &Settings) -> Result<(), String> {
        // A manual "next" always moves on, even in Repeat One (which only affects end-of-track).
        let policy = if self.repeat == RepeatMode::One {
            RepeatMode::Off
        } else {
            self.repeat
        };
        match next_index(policy, self.index, self.queue.len()) {
            Some(index) => {
                self.index = index;
                self.resume_secs = None;
                self.pending_seek_secs = None;
                self.start_current(library, engine, settings)
            }
            None => {
                self.stop(engine);
                Ok(())
            }
        }
    }

    pub fn previous(&mut self, library: &MusicLibrary, engine: &Engine, settings: &Settings) -> Result<(), String> {
        if self.queue.is_empty() {
            return Err("Nothing is queued.".to_string());
        }
        if self.index > 0 {
            self.index -= 1;
        } else if self.repeat == RepeatMode::All {
            self.index = self.queue.len() - 1;
        }
        self.resume_secs = None;
        self.pending_seek_secs = None;
        self.start_current(library, engine, settings)
    }

    /// Jump to a fraction (0..=1) of the current track.
    pub fn seek_frac(&mut self, engine: &Engine, frac: f32) {
        let frac = frac.clamp(0.0, 1.0);
        match self.state {
            PlaybackState::Playing => {
                engine.seek(&self.sound_id, frac);
                self.last_pos = frac as f64 * self.last_total;
            }
            PlaybackState::Loading => {
                // The engine may not have started the voice yet; apply the jump when it does.
                let target = frac as f64 * self.last_total;
                if !engine.seek(&self.sound_id, frac) {
                    self.pending_seek_secs = Some(target);
                }
                self.last_pos = target;
            }
            PlaybackState::Paused => {
                let pos = frac as f64 * self.last_total;
                self.resume_secs = Some(pos);
                self.last_pos = pos;
            }
            _ => {}
        }
    }

    /// Start or resume the queue item at `index`.
    fn start_current(&mut self, library: &MusicLibrary, engine: &Engine, settings: &Settings) -> Result<(), String> {
        let Some(track_id) = self.queue.get(self.index).cloned() else {
            self.state = PlaybackState::Idle;
            return Err("Nothing is queued.".to_string());
        };
        let Some(track) = library.track(&track_id) else {
            self.state = PlaybackState::Idle;
            return Err("The queued track is no longer in the music library.".to_string());
        };
        self.current = Some(track_id.clone());
        self.last_total = track.duration_secs;
        self.error = None;
        self.loading_since = Some(Instant::now());

        if !track.available() {
            self.state = PlaybackState::Buffering;
            self.last_pos = 0.0;
            return Ok(());
        }
        let Some(path) = track.path.as_deref() else {
            self.state = PlaybackState::Buffering;
            return Ok(());
        };
        self.stop_sound(engine);
        let sound_id = format!("{MUSIC_SOUND_PREFIX}{}", track.id);
        engine.play_with_limit(
            &sound_id,
            path,
            settings.music.volume,
            crate::settings::RetriggerPolicy::Restart,
            true,
            MAX_MUSIC_SECS,
        );
        self.sound_id = sound_id;
        self.state = PlaybackState::Loading;
        self.last_pos = 0.0;
        Ok(())
    }

    fn stop_sound(&self, engine: &Engine) {
        if !self.sound_id.is_empty() {
            engine.stop(&self.sound_id);
        }
    }

    /// Called by the app on every frame (and often enough while busy).
    pub fn tick(&mut self, library: &MusicLibrary, engine: &Engine, settings: &Settings) {
        match self.state {
            PlaybackState::Loading => {
                if let Some((pos, total)) = engine.progress(&self.sound_id) {
                    if total > 0.0 {
                        self.last_total = total;
                    }
                    self.last_pos = pos;
                    if let Some(target) = self.pending_seek_secs.take() {
                        if self.last_total > 0.0 {
                            engine.seek(&self.sound_id, (target / self.last_total) as f32);
                            self.last_pos = target;
                        }
                    }
                    self.state = PlaybackState::Playing;
                    self.resume_secs = None;
                } else if self
                    .loading_since
                    .is_some_and(|t| t.elapsed() > loading_timeout(self.last_total))
                {
                    self.stop_sound(engine);
                    self.state = PlaybackState::Error;
                    self.error =
                        Some("The track could not be decoded. The file may be damaged; download it again.".to_string());
                }
            }
            PlaybackState::Playing => {
                match engine.progress(&self.sound_id) {
                    Some((pos, total)) => {
                        if total > 0.0 {
                            self.last_total = total;
                        }
                        self.last_pos = pos;
                    }
                    None => {
                        let total = self.last_total;
                        let threshold = (total * 0.02).clamp(0.5, 3.0);
                        if total > 0.0 && self.last_pos + threshold >= total {
                            self.advance_after_finish(library, engine, settings);
                        } else {
                            // Stopped from outside (Stop All, device change, ...): pause here.
                            self.state = PlaybackState::Paused;
                            self.resume_secs = Some(self.last_pos);
                            self.pending_seek_secs = None;
                        }
                    }
                }
            }
            PlaybackState::Buffering => {
                let Some(track) = self.current_track(library) else {
                    self.state = PlaybackState::Error;
                    self.error = Some("The queued track disappeared from the music library.".to_string());
                    return;
                };
                if track.available() {
                    let _ = self.start_current(library, engine, settings);
                } else if track.status == TrackStatus::Failed {
                    self.state = PlaybackState::Error;
                    self.error = Some(
                        track
                            .detail
                            .clone()
                            .unwrap_or_else(|| "This track could not be downloaded.".to_string()),
                    );
                }
            }
            _ => {}
        }
    }

    fn advance_after_finish(&mut self, library: &MusicLibrary, engine: &Engine, settings: &Settings) {
        match self.repeat {
            RepeatMode::One => {
                self.resume_secs = None;
                self.pending_seek_secs = None;
                let _ = self.start_current(library, engine, settings);
            }
            RepeatMode::All => {
                if self.queue.is_empty() {
                    self.stop(engine);
                    return;
                }
                self.index = (self.index + 1) % self.queue.len();
                self.resume_secs = None;
                self.pending_seek_secs = None;
                let _ = self.start_current(library, engine, settings);
            }
            RepeatMode::Off => {
                if self.index + 1 < self.queue.len() {
                    self.index += 1;
                    self.resume_secs = None;
                    self.pending_seek_secs = None;
                    let _ = self.start_current(library, engine, settings);
                } else {
                    self.state = PlaybackState::Idle;
                    self.current = None;
                    self.last_pos = 0.0;
                    self.resume_secs = None;
                    self.pending_seek_secs = None;
                }
            }
        }
    }

    /// The engine reported an error for a music sound.
    pub fn on_engine_error(&mut self, sound_id: &str, message: String) {
        if sound_id != self.sound_id {
            return;
        }
        self.state = PlaybackState::Error;
        self.error = Some(message);
        self.loading_since = None;
    }

    /// Downloading the current track failed; show it in the player instead of spinning forever.
    pub fn on_download_failed(&mut self, track_id: &str, message: String) {
        if self.current.as_deref() != Some(track_id) {
            return;
        }
        self.state = PlaybackState::Error;
        self.error = Some(message);
        self.loading_since = None;
    }

    /// The current track became ready while the player was waiting for it.
    pub fn retry_waiting(&mut self, library: &MusicLibrary, engine: &Engine, settings: &Settings) {
        if self.state == PlaybackState::Buffering || self.state == PlaybackState::Error {
            self.state = PlaybackState::Buffering;
            self.error = None;
            if self
                .current
                .as_deref()
                .and_then(|id| library.track(id))
                .is_some_and(|t| t.available())
            {
                let _ = self.start_current(library, engine, settings);
            }
        }
    }
}

/// Index to play after the current track ends, or `None` when the queue stops.
pub fn next_index(repeat: RepeatMode, index: usize, len: usize) -> Option<usize> {
    if len == 0 {
        return None;
    }
    match repeat {
        RepeatMode::One => Some(index.min(len - 1)),
        RepeatMode::All => Some((index + 1) % len),
        RepeatMode::Off => {
            if index + 1 < len {
                Some(index + 1)
            } else {
                None
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_cycles_through_three_modes() {
        assert_eq!(RepeatMode::Off.next(), RepeatMode::All);
        assert_eq!(RepeatMode::All.next(), RepeatMode::One);
        assert_eq!(RepeatMode::One.next(), RepeatMode::Off);
    }

    #[test]
    fn off_stops_after_the_last_track() {
        assert_eq!(next_index(RepeatMode::Off, 0, 3), Some(1));
        assert_eq!(next_index(RepeatMode::Off, 2, 3), None);
        assert_eq!(next_index(RepeatMode::Off, 0, 0), None);
    }

    #[test]
    fn all_wraps_and_one_stays() {
        assert_eq!(next_index(RepeatMode::All, 2, 3), Some(0));
        assert_eq!(next_index(RepeatMode::One, 1, 3), Some(1));
        assert_eq!(next_index(RepeatMode::One, 9, 3), Some(2));
    }

    #[test]
    fn player_starts_idle_with_no_queue() {
        let p = MusicPlayer::default();
        assert_eq!(p.state(), PlaybackState::Idle);
        assert_eq!(p.time(), (0.0, 0.0));
        assert!(p.current().is_none());
    }

    #[test]
    fn the_decoding_timeout_grows_with_the_track() {
        let short = loading_timeout(30.0);
        let long = loading_timeout(600.0);
        assert!(long > short, "a long track needs more time to decode");
        assert!(short >= LOADING_TIMEOUT, "the base limit is always granted");
        assert_eq!(loading_timeout(f64::NAN), LOADING_TIMEOUT);
        assert_eq!(loading_timeout(10_000.0), LOADING_TIMEOUT + Duration::from_secs(600));
    }
}
