//! Music library: albums imported from Spotify, their tracks and download state.
//!
//! Audio files live in a separate cache directory (`AppPaths::music_dir`) and never mix with the
//! soundboard library: a track is minutes long, so it is decoded on demand and only while it is
//! the current queue item. `music.json` stores metadata only; a track whose file disappeared is
//! put back into the queue instead of pretending to be playable.

use crate::errors::StorageError;
use crate::fsutil::{backup_corrupt, load_json, save_json, unix_now, write_atomic, LoadStatus};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

pub const MUSIC_VERSION: u32 = 1;

/// Where a track is in the download pipeline.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, Default)]
#[serde(rename_all = "snake_case")]
pub enum TrackStatus {
    /// Waiting for the download worker.
    #[default]
    Queued,
    /// yt-dlp is looking for it / downloading it right now.
    Downloading,
    /// The audio file is in the cache and can be played.
    Ready,
    /// No source matched; `detail` says why.
    Failed,
}

impl TrackStatus {
    pub fn is_ready(self) -> bool {
        matches!(self, TrackStatus::Ready)
    }
}

/// One track of an imported album.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MusicTrack {
    /// Local id; also the base name of the cached audio file.
    pub id: String,
    /// Spotify track id, when known.
    #[serde(default)]
    pub spotify_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub artist: String,
    /// Album title (empty for a track imported on its own).
    #[serde(default)]
    pub album: String,
    /// Length reported by Spotify, in seconds. Used to reject remixes and clips.
    #[serde(default)]
    pub duration_secs: f64,
    /// Cached audio file, once downloaded.
    #[serde(default)]
    pub path: Option<PathBuf>,
    #[serde(default)]
    pub status: TrackStatus,
    /// Human-readable reason for the current status (a download error, a mismatch, ...).
    #[serde(default)]
    pub detail: Option<String>,
    /// `soundcloud` or `youtube`, set after a successful download.
    #[serde(default)]
    pub source: Option<String>,
    /// Page the audio actually came from.
    #[serde(default)]
    pub source_url: Option<String>,
}

impl MusicTrack {
    /// A new, not yet downloaded track.
    pub fn new(
        title: impl Into<String>,
        artist: impl Into<String>,
        album: impl Into<String>,
        duration_secs: f64,
        spotify_id: Option<String>,
    ) -> MusicTrack {
        MusicTrack {
            id: uuid::Uuid::new_v4().to_string(),
            spotify_id,
            title: title.into(),
            artist: artist.into(),
            album: album.into(),
            duration_secs: if duration_secs.is_finite() {
                duration_secs.max(0.0)
            } else {
                0.0
            },
            path: None,
            status: TrackStatus::Queued,
            detail: None,
            source: None,
            source_url: None,
        }
    }

    /// True when the cached file exists and is not empty.
    pub fn available(&self) -> bool {
        self.status.is_ready()
            && self
                .path
                .as_ref()
                .is_some_and(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() > 0))
    }

    /// Align the status with what is actually on disk (used at startup and after a crash).
    pub fn refresh(&mut self) {
        let on_disk = self
            .path
            .as_ref()
            .is_some_and(|p| fs::metadata(p).is_ok_and(|m| m.is_file() && m.len() > 0));
        match self.status {
            // A download that was running when the app closed is simply queued again.
            TrackStatus::Downloading => {
                if on_disk {
                    self.status = TrackStatus::Ready;
                } else {
                    self.status = TrackStatus::Queued;
                    self.path = None;
                    self.detail = None;
                }
            }
            TrackStatus::Ready if !on_disk => {
                self.status = TrackStatus::Queued;
                self.path = None;
                self.detail = Some("the cached audio file is missing".to_string());
            }
            // A file may have been written just before a crash, before the status was saved.
            TrackStatus::Queued | TrackStatus::Failed if on_disk => {
                self.status = TrackStatus::Ready;
                self.detail = None;
            }
            _ => {}
        }
        if self.title.trim().is_empty() {
            self.title = "Unknown track".to_string();
        }
        if !self.duration_secs.is_finite() || self.duration_secs < 0.0 {
            self.duration_secs = 0.0;
        }
    }

    /// "Artist - Title", which is the search query used by the downloader.
    pub fn search_query(&self) -> String {
        let artist = self.artist.trim();
        let title = self.title.trim();
        if artist.is_empty() || title.to_lowercase().contains(&artist.to_lowercase()) {
            title.to_string()
        } else {
            format!("{artist} - {title}")
        }
    }
}

/// An imported album (or a single track, which is stored as a one-track album).
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
pub struct MusicAlbum {
    pub id: String,
    #[serde(default)]
    pub spotify_id: Option<String>,
    pub title: String,
    #[serde(default)]
    pub artist: String,
    #[serde(default)]
    pub cover_url: Option<String>,
    /// The Spotify link the album was imported from.
    #[serde(default)]
    pub source_url: String,
    #[serde(default)]
    pub added_at: u64,
    #[serde(default)]
    pub tracks: Vec<MusicTrack>,
}

impl MusicAlbum {
    pub fn duration_secs(&self) -> f64 {
        self.tracks.iter().map(|t| t.duration_secs).sum()
    }

    pub fn ready_count(&self) -> usize {
        self.tracks.iter().filter(|t| t.available()).count()
    }

    pub fn track_index(&self, track_id: &str) -> Option<usize> {
        self.tracks.iter().position(|t| t.id == track_id)
    }

    pub fn refresh(&mut self) {
        if self.title.trim().is_empty() {
            self.title = "Untitled album".to_string();
        }
        for t in &mut self.tracks {
            t.refresh();
        }
    }
}

/// Everything shown in the Music section, persisted as `music.json`.
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(default)]
pub struct MusicLibrary {
    pub version: u32,
    pub albums: Vec<MusicAlbum>,
}

impl Default for MusicLibrary {
    fn default() -> Self {
        MusicLibrary {
            version: MUSIC_VERSION,
            albums: Vec::new(),
        }
    }
}

impl MusicLibrary {
    /// Load `music.json` entry by entry: one damaged album does not hide the rest.
    pub fn load(path: &Path) -> Result<(MusicLibrary, LoadStatus), StorageError> {
        use serde_json::Value;
        let (value, status): (Value, LoadStatus) = load_json(path)?;
        let map = match value {
            Value::Object(map) => map,
            Value::Null => return Ok((MusicLibrary::default(), status)),
            _ => {
                let backup = backup_corrupt(path);
                let reason = "the file does not contain a music library object".to_string();
                return Ok((MusicLibrary::default(), LoadStatus::Recovered { backup, reason }));
            }
        };
        let raw: Vec<Value> = match map.get("albums") {
            Some(Value::Array(a)) => a.clone(),
            _ => Vec::new(),
        };
        let mut albums: Vec<MusicAlbum> = Vec::with_capacity(raw.len());
        let mut seen_albums: HashSet<String> = HashSet::new();
        let mut seen_tracks: HashSet<String> = HashSet::new();
        let mut skipped: Vec<Value> = Vec::new();
        for item in raw {
            match serde_json::from_value::<MusicAlbum>(item.clone()) {
                Ok(mut album) if !album.id.is_empty() && seen_albums.insert(album.id.clone()) => {
                    album
                        .tracks
                        .retain(|t| !t.id.is_empty() && seen_tracks.insert(t.id.clone()));
                    album.refresh();
                    albums.push(album);
                }
                _ => skipped.push(item),
            }
        }
        let lib = MusicLibrary {
            version: MUSIC_VERSION,
            albums,
        };
        if skipped.is_empty() {
            return Ok((lib, status));
        }
        let mut name = path.file_name().map(|n| n.to_os_string()).unwrap_or_default();
        name.push(format!(".skipped-{}", unix_now()));
        let side = path.with_file_name(name);
        let backup = match serde_json::to_vec_pretty(&skipped) {
            Ok(bytes) => write_atomic(&side, &bytes).ok().map(|_| side),
            Err(_) => None,
        };
        let reason = format!("{} damaged music album(s) were skipped", skipped.len());
        Ok((lib, LoadStatus::Recovered { backup, reason }))
    }

    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        save_json(path, self)
    }

    pub fn add_album(&mut self, album: MusicAlbum) -> String {
        let id = album.id.clone();
        self.albums.push(album);
        id
    }

    pub fn get(&self, album_id: &str) -> Option<&MusicAlbum> {
        self.albums.iter().find(|a| a.id == album_id)
    }

    pub fn get_mut(&mut self, album_id: &str) -> Option<&mut MusicAlbum> {
        self.albums.iter_mut().find(|a| a.id == album_id)
    }

    pub fn remove(&mut self, album_id: &str) -> Option<MusicAlbum> {
        let idx = self.albums.iter().position(|a| a.id == album_id)?;
        Some(self.albums.remove(idx))
    }

    pub fn find_by_spotify(&self, spotify_id: &str) -> Option<&MusicAlbum> {
        self.albums.iter().find(|a| a.spotify_id.as_deref() == Some(spotify_id))
    }

    /// Album that contains `track_id`.
    pub fn album_of_track(&self, track_id: &str) -> Option<&MusicAlbum> {
        self.albums.iter().find(|a| a.track_index(track_id).is_some())
    }

    pub fn track(&self, track_id: &str) -> Option<&MusicTrack> {
        self.albums
            .iter()
            .find_map(|a| a.tracks.iter().find(|t| t.id == track_id))
    }

    pub fn track_mut(&mut self, track_id: &str) -> Option<&mut MusicTrack> {
        self.albums
            .iter_mut()
            .find_map(|a| a.tracks.iter_mut().find(|t| t.id == track_id))
    }

    /// Change a track's status; returns true when something changed.
    pub fn set_status(&mut self, track_id: &str, status: TrackStatus, detail: Option<String>) -> bool {
        let Some(track) = self.track_mut(track_id) else {
            return false;
        };
        let changed = track.status != status || track.detail != detail;
        track.status = status;
        track.detail = detail;
        changed
    }

    /// Remember a successful download.
    pub fn set_ready(
        &mut self,
        track_id: &str,
        path: PathBuf,
        source: Option<String>,
        source_url: Option<String>,
        duration_secs: Option<f64>,
    ) -> bool {
        let Some(track) = self.track_mut(track_id) else {
            return false;
        };
        track.status = TrackStatus::Ready;
        track.detail = None;
        track.path = Some(path);
        if source.is_some() {
            track.source = source;
        }
        if source_url.is_some() {
            track.source_url = source_url;
        }
        if let Some(d) = duration_secs.filter(|d| d.is_finite() && *d > 0.0) {
            track.duration_secs = d;
        }
        true
    }

    pub fn track_count(&self) -> usize {
        self.albums.iter().map(|a| a.tracks.len()).sum()
    }

    pub fn ready_count(&self) -> usize {
        self.albums.iter().map(|a| a.ready_count()).sum()
    }

    /// Re-check every track against the cache directory (after a restart or a manual clean-up).
    pub fn refresh(&mut self) {
        self.version = MUSIC_VERSION;
        for album in &mut self.albums {
            album.refresh();
        }
    }

    /// Total size of the cached audio files.
    pub fn cache_bytes(&self) -> u64 {
        self.albums
            .iter()
            .flat_map(|a| a.tracks.iter())
            .filter_map(|t| t.path.as_ref())
            .filter_map(|p| fs::metadata(p).ok())
            .map(|m| m.len())
            .sum()
    }

    /// Delete every cached audio file and put all tracks back into the queue.
    ///
    /// Only direct children of `dir` are removed, which is the directory VibeDrop owns. The
    /// caller passes `AppPaths::music_dir`, never a user folder.
    pub fn clear_cache(&mut self, dir: &Path) -> Result<usize, String> {
        let mut removed = 0usize;
        if let Ok(entries) = fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && fs::remove_file(&path).is_ok() {
                    removed += 1;
                }
            }
        }
        for album in &mut self.albums {
            for track in &mut album.tracks {
                track.path = None;
                track.status = TrackStatus::Queued;
                track.detail = None;
            }
        }
        Ok(removed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn album() -> MusicAlbum {
        let mut a = MusicAlbum {
            id: "album-1".to_string(),
            spotify_id: Some("sp-1".to_string()),
            title: "Test album".to_string(),
            artist: "Artist".to_string(),
            cover_url: None,
            source_url: "https://open.spotify.com/album/sp-1".to_string(),
            added_at: 1,
            tracks: Vec::new(),
        };
        a.tracks
            .push(MusicTrack::new("One", "Artist", "Test album", 61.5, Some("t1".into())));
        a.tracks
            .push(MusicTrack::new("Two", "Artist", "Test album", 62.0, Some("t2".into())));
        a
    }

    #[test]
    fn query_uses_artist_and_title_once() {
        let t = MusicTrack::new("Song", "Artist", "", 10.0, None);
        assert_eq!(t.search_query(), "Artist - Song");
        let t = MusicTrack::new("Artist - Song", "Artist", "", 10.0, None);
        assert_eq!(t.search_query(), "Artist - Song");
        let t = MusicTrack::new("Song", "", "", 10.0, None);
        assert_eq!(t.search_query(), "Song");
    }

    #[test]
    fn load_and_save_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("music.json");
        let mut lib = MusicLibrary::default();
        lib.add_album(album());
        lib.save(&path).unwrap();
        let (loaded, status) = MusicLibrary::load(&path).unwrap();
        assert_eq!(status, LoadStatus::Loaded);
        assert_eq!(loaded.albums.len(), 1);
        assert_eq!(loaded.track_count(), 2);
        assert_eq!(loaded.albums[0].duration_secs(), 123.5);
    }

    #[test]
    fn missing_cache_file_requeues_a_ready_track() {
        let dir = tempfile::tempdir().unwrap();
        let mut lib = MusicLibrary::default();
        let mut a = album();
        a.tracks[0].status = TrackStatus::Ready;
        a.tracks[0].path = Some(dir.path().join("gone.mp3"));
        lib.add_album(a);
        lib.refresh();
        assert_eq!(lib.albums[0].tracks[0].status, TrackStatus::Queued);
        assert!(lib.albums[0].tracks[0].path.is_none());
    }

    #[test]
    fn interrupted_download_is_requeued_and_existing_file_is_adopted() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("t.mp3");
        fs::write(&file, b"ID3fake").unwrap();
        let mut lib = MusicLibrary::default();
        let mut a = album();
        a.tracks[0].status = TrackStatus::Downloading;
        a.tracks[0].path = Some(file.clone());
        a.tracks[1].status = TrackStatus::Downloading;
        a.tracks[1].path = Some(dir.path().join("missing.mp3"));
        lib.add_album(a);
        lib.refresh();
        assert_eq!(lib.albums[0].tracks[0].status, TrackStatus::Ready);
        assert_eq!(lib.albums[0].tracks[1].status, TrackStatus::Queued);
        assert!(lib.albums[0].tracks[1].path.is_none());
    }

    #[test]
    fn clear_cache_removes_files_and_requeues() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("one.mp3");
        fs::write(&file, b"ID3fake").unwrap();
        let mut lib = MusicLibrary::default();
        let mut a = album();
        a.tracks[0].status = TrackStatus::Ready;
        a.tracks[0].path = Some(file.clone());
        lib.add_album(a);
        assert_eq!(lib.clear_cache(dir.path()).unwrap(), 1);
        assert!(!file.exists());
        assert_eq!(lib.albums[0].tracks[0].status, TrackStatus::Queued);
        assert_eq!(lib.cache_bytes(), 0);
    }

    #[test]
    fn damaged_album_is_skipped_but_others_load() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("music.json");
        let good = serde_json::to_value(album()).unwrap();
        let data = serde_json::json!({"version": 1, "albums": [good, {"id": 42}]});
        fs::write(&path, serde_json::to_vec(&data).unwrap()).unwrap();
        let (lib, status) = MusicLibrary::load(&path).unwrap();
        assert_eq!(lib.albums.len(), 1);
        assert!(matches!(status, LoadStatus::Recovered { .. }));
    }
}
