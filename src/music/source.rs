//! Music sources other than Spotify: SoundCloud, YouTube and direct audio links.
//!
//! The metadata comes from yt-dlp itself (it prints the page as JSON), so nothing has to be
//! scraped and every link that yt-dlp can download can be imported here as well.

use crate::music::tools::{self, Tools};
use std::ffi::OsString;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// How long the metadata query may take.
const META_TIMEOUT: Duration = Duration::from_secs(90);
/// How many tracks of a collection are imported at most.
const MAX_TRACKS: usize = 200;

/// The kind of link the user pasted.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SourceKind {
    SoundCloud,
    YouTube,
    DirectFile,
}

impl SourceKind {
    pub fn label(self) -> &'static str {
        match self {
            SourceKind::SoundCloud => "SoundCloud",
            SourceKind::YouTube => "YouTube",
            SourceKind::DirectFile => "Direct link",
        }
    }

    /// The name of the collection kind in the music library.
    pub fn collection_label(self) -> &'static str {
        match self {
            SourceKind::SoundCloud => "SoundCloud",
            SourceKind::YouTube => "YouTube",
            SourceKind::DirectFile => "Track",
        }
    }
}

/// A link that is not a Spotify link.
#[derive(Clone, Debug)]
pub struct SourceLink {
    pub kind: SourceKind,
    pub url: String,
}

/// One track of a fetched collection.
#[derive(Clone, Debug)]
pub struct FetchedSourceTrack {
    pub title: String,
    pub artist: String,
    pub album: String,
    pub duration_secs: f64,
    /// The page the track is downloaded from.
    pub url: String,
}

/// A fetched collection: one track or a list of them.
#[derive(Clone, Debug)]
pub struct FetchedSource {
    pub title: String,
    pub artist: String,
    pub cover_url: Option<String>,
    pub tracks: Vec<FetchedSourceTrack>,
}

/// Recognise a link that yt-dlp can handle.
pub fn parse_link(input: &str) -> Result<SourceLink, String> {
    let text = input.trim();
    if text.is_empty() {
        return Err("The link is empty.".to_string());
    }
    let url = if text.contains("://") {
        text.to_string()
    } else {
        format!("https://{text}")
    };
    let lower = url.to_lowercase();
    if lower.contains("soundcloud.com/") {
        return Ok(SourceLink {
            kind: SourceKind::SoundCloud,
            url,
        });
    }
    if lower.contains("youtube.com/") || lower.contains("youtu.be/") {
        return Ok(SourceLink {
            kind: SourceKind::YouTube,
            url,
        });
    }
    if lower.ends_with(".mp3")
        || lower.ends_with(".m4a")
        || lower.ends_with(".wav")
        || lower.ends_with(".flac")
        || lower.ends_with(".ogg")
        || lower.ends_with(".opus")
        || lower.ends_with(".aac")
    {
        return Ok(SourceLink {
            kind: SourceKind::DirectFile,
            url,
        });
    }
    Err(
        "This is not a Spotify, SoundCloud or YouTube link. VibeDrop also accepts a direct link to an audio file."
            .to_string(),
    )
}

/// Ask yt-dlp for the metadata of the link.
pub fn fetch(link: &SourceLink, tools: &Tools) -> Result<FetchedSource, String> {
    let Some(yt_dlp) = tools.yt_dlp.as_deref() else {
        return Err("yt-dlp is needed to read the metadata of this link.".to_string());
    };
    let cancel = AtomicBool::new(false);
    let current = tools::new_running_child();
    let mut args: Vec<OsString> = vec![
        "--ignore-config".into(),
        "--no-warnings".into(),
        "--dump-single-json".into(),
    ];
    args.push(link.url.clone().into());
    let result = tools::run_capture(yt_dlp, &args, META_TIMEOUT, &cancel, &current)?;
    if !result.success() {
        let tail = result.error_tail();
        return Err(if tail.is_empty() {
            "yt-dlp could not read this link.".to_string()
        } else {
            tail
        });
    }
    let value: serde_json::Value =
        serde_json::from_str(result.stdout.trim()).map_err(|e| format!("yt-dlp returned something unexpected: {e}"))?;
    parse_metadata(&value)
}

/// Turn the JSON of yt-dlp into a collection.
fn parse_metadata(value: &serde_json::Value) -> Result<FetchedSource, String> {
    let entries = value.get("entries").and_then(|e| e.as_array());
    match entries {
        Some(list) if !list.is_empty() => {
            let title = text(value, "title").unwrap_or_else(|| "Playlist".to_string());
            let artist = text(value, "uploader")
                .or_else(|| text(value, "channel"))
                .unwrap_or_default();
            let cover = cover_of(value);
            let mut tracks = Vec::new();
            for entry in list.iter().take(MAX_TRACKS) {
                let Some(url) = entry_url(entry) else {
                    continue;
                };
                let title = text(entry, "title").unwrap_or_else(|| "Unknown".to_string());
                let artist = text(entry, "uploader")
                    .or_else(|| text(entry, "channel"))
                    .or_else(|| Some(artist.clone()))
                    .unwrap_or_default();
                let album = title.clone();
                tracks.push(FetchedSourceTrack {
                    title,
                    artist,
                    album,
                    duration_secs: number(entry, "duration").unwrap_or(0.0),
                    url,
                });
            }
            if tracks.is_empty() {
                return Err("This link did not contain any tracks.".to_string());
            }
            Ok(FetchedSource {
                title,
                artist,
                cover_url: cover,
                tracks,
            })
        }
        _ => {
            let Some(url) = entry_url(value) else {
                return Err("This link does not point to a track.".to_string());
            };
            let title = text(value, "title").unwrap_or_else(|| "Unknown".to_string());
            let artist = text(value, "uploader")
                .or_else(|| text(value, "channel"))
                .unwrap_or_default();
            Ok(FetchedSource {
                title: title.clone(),
                artist: artist.clone(),
                cover_url: cover_of(value),
                tracks: vec![FetchedSourceTrack {
                    title,
                    artist,
                    album: String::new(),
                    duration_secs: number(value, "duration").unwrap_or(0.0),
                    url,
                }],
            })
        }
    }
}

/// The address of a track: the page it came from.
fn entry_url(value: &serde_json::Value) -> Option<String> {
    if let Some(url) = text(value, "webpage_url") {
        return Some(url);
    }
    if let Some(url) = text(value, "original_url") {
        return Some(url);
    }
    // A playlist entry of YouTube only has the video id.
    if let Some(id) = text(value, "id") {
        if id.len() == 11 && !id.starts_with("http") {
            return Some(format!("https://www.youtube.com/watch?v={id}"));
        }
    }
    text(value, "url")
}

fn text(value: &serde_json::Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn number(value: &serde_json::Value, key: &str) -> Option<f64> {
    value.get(key).and_then(|v| v.as_f64())
}

/// The best picture of a page.
fn cover_of(value: &serde_json::Value) -> Option<String> {
    if let Some(list) = value.get("thumbnails").and_then(|t| t.as_array()) {
        if let Some(best) = list.last().and_then(|t| text(t, "url")) {
            return Some(best);
        }
    }
    text(value, "thumbnail")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soundcloud_and_youtube_links_are_recognised() {
        let sc = parse_link("https://soundcloud.com/artist/track").expect("soundcloud");
        assert_eq!(sc.kind, SourceKind::SoundCloud);
        let yt = parse_link("https://youtu.be/abcdefghijk").expect("youtube");
        assert_eq!(yt.kind, SourceKind::YouTube);
        let file = parse_link("https://example.com/song.mp3").expect("file");
        assert_eq!(file.kind, SourceKind::DirectFile);
    }

    #[test]
    fn a_scheme_is_added_when_it_is_missing() {
        let link = parse_link("soundcloud.com/artist/track").expect("soundcloud");
        assert!(link.url.starts_with("https://"));
    }

    #[test]
    fn other_links_are_refused() {
        assert!(parse_link("https://example.com/page").is_err());
        assert!(parse_link("").is_err());
    }

    #[test]
    fn a_single_track_becomes_a_collection_of_one() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"title":"Song","uploader":"Artist","duration":123.0,
                "webpage_url":"https://soundcloud.com/a/song","thumbnail":"https://img/1.jpg"}"#,
        )
        .expect("json");
        let source = parse_metadata(&value).expect("collection");
        assert_eq!(source.tracks.len(), 1);
        assert_eq!(source.tracks[0].title, "Song");
        assert_eq!(source.tracks[0].artist, "Artist");
        assert_eq!(source.tracks[0].duration_secs, 123.0);
        assert_eq!(source.cover_url.as_deref(), Some("https://img/1.jpg"));
    }

    #[test]
    fn a_playlist_keeps_its_tracks() {
        let value: serde_json::Value = serde_json::from_str(
            r#"{"title":"Set","uploader":"Artist","entries":[
                {"title":"One","uploader":"A","duration":10.0,"webpage_url":"https://soundcloud.com/a/one"},
                {"title":"Two","uploader":"B","duration":20.0,"webpage_url":"https://soundcloud.com/a/two"}]}"#,
        )
        .expect("json");
        let source = parse_metadata(&value).expect("collection");
        assert_eq!(source.tracks.len(), 2);
        assert_eq!(source.title, "Set");
        assert_eq!(source.tracks[1].title, "Two");
    }

    #[test]
    fn a_youtube_entry_without_a_page_gets_its_address() {
        let value: serde_json::Value =
            serde_json::from_str(r#"{"title":"Video","id":"abcdefghijk","duration":5.0}"#).expect("json");
        let source = parse_metadata(&value).expect("collection");
        assert_eq!(source.tracks[0].url, "https://www.youtube.com/watch?v=abcdefghijk");
    }
}
