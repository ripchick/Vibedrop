//! Background downloader for the Music section.
//!
//! One worker thread processes one track at a time (searching and downloading in parallel would
//! only make both slower and get the IP blocked). The cascade is:
//!
//! 1. `scsearch1:` on SoundCloud for "Artist - Title"; the found track is accepted only when its
//!    duration differs from Spotify's by at most [`SOUNDCLOUD_TOLERANCE`] seconds, which filters
//!    out remixes, covers and 30-second clips.
//! 2. `ytsearch1:` on YouTube with "official audio" appended when SoundCloud has no match or the
//!    duration is wrong. The tolerance is slightly wider because YouTube uploads often include a
//!    second of silence.
//!
//! The result is converted to MP3 by `yt-dlp`/`ffmpeg` in the music cache. Every event carries the
//! track id so the UI can show a per-track status; the worker is cancellable and never runs a
//! shell.

use crate::audio::engine::MAX_MUSIC_SECS;
use crate::library::validate_audio_file;
use crate::music::library::MusicTrack;
use crate::music::spotify::clean_title;
use crate::music::tools::{self, RunResult, RunningChild, Tools};
use crossbeam_channel::{unbounded, Receiver, RecvTimeoutError, Sender};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

/// SoundCloud duration tolerance from the spec.
pub const SOUNDCLOUD_TOLERANCE: f64 = 15.0;
/// YouTube tolerance: official uploads can differ by a couple of seconds.
pub const YOUTUBE_TOLERANCE: f64 = 20.0;

const SEARCH_TIMEOUT: Duration = Duration::from_secs(120);
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(15 * 60);

#[derive(Debug, Clone)]
pub struct DownloadJob {
    pub track_id: String,
    pub title: String,
    pub artist: String,
    pub duration_secs: f64,
    /// Final MP3 path: `<music_dir>/<track_id>.mp3`.
    pub dest: PathBuf,
}

impl DownloadJob {
    pub fn from_track(track: &MusicTrack, dest: PathBuf) -> DownloadJob {
        DownloadJob {
            track_id: track.id.clone(),
            title: track.title.clone(),
            artist: track.artist.clone(),
            duration_secs: track.duration_secs,
            dest,
        }
    }

    /// "Artist - Title" without remaster/edition qualifiers.
    fn query(&self) -> String {
        let title = clean_title(&self.title);
        let artist = self.artist.trim();
        if artist.is_empty() || title.to_lowercase().contains(&artist.to_lowercase()) {
            title
        } else {
            format!("{artist} - {title}")
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    SoundCloud,
    YouTube,
}

impl Stage {
    pub fn label(self) -> &'static str {
        match self {
            Stage::SoundCloud => "SoundCloud",
            Stage::YouTube => "YouTube",
        }
    }
}

#[derive(Debug, Clone)]
pub struct DownloadedTrack {
    pub path: PathBuf,
    pub source: String,
    pub source_url: String,
    pub duration_secs: Option<f64>,
}

#[derive(Debug, Clone)]
pub enum DownloadResult {
    Ready(DownloadedTrack),
    Cancelled,
    Failed(String),
}

#[derive(Debug, Clone)]
pub enum DownloadEvent {
    /// The worker started working on the track and is currently looking at `stage`.
    Started {
        track_id: String,
        stage: Stage,
    },
    Finished {
        track_id: String,
        result: DownloadResult,
    },
}

/// Start the single download worker. `tools` is shared so a path change in Settings takes effect
/// for the next job without restarting the app.
pub fn start(dest_dir: PathBuf, tools: Tools) -> DownloadQueue {
    DownloadQueue::start(dest_dir, tools)
}

pub struct DownloadQueue {
    tx: Sender<Msg>,
    events: Receiver<DownloadEvent>,
    generation: Arc<AtomicU64>,
    tools: Arc<Mutex<Tools>>,
    cancel: Arc<AtomicBool>,
    current: RunningChild,
    join: Option<std::thread::JoinHandle<()>>,
}

enum Msg {
    Job(DownloadJob, u64),
    Shutdown,
}

impl DownloadQueue {
    pub fn start(dest_dir: PathBuf, tools: Tools) -> DownloadQueue {
        let (tx, rx) = unbounded::<Msg>();
        let (etx, erx) = unbounded::<DownloadEvent>();
        let generation = Arc::new(AtomicU64::new(0));
        let tools = Arc::new(Mutex::new(tools));
        let cancel = Arc::new(AtomicBool::new(false));
        let current = tools::new_running_child();

        let worker_generation = Arc::clone(&generation);
        let worker_tools = Arc::clone(&tools);
        let worker_cancel = Arc::clone(&cancel);
        let worker_current = Arc::clone(&current);
        let join = std::thread::Builder::new()
            .name("vibedrop-music-download".to_string())
            .spawn(move || loop {
                let msg = match rx.recv_timeout(Duration::from_millis(250)) {
                    Ok(msg) => msg,
                    // An idle worker must stay alive: the next import may come minutes later.
                    Err(RecvTimeoutError::Timeout) => continue,
                    Err(RecvTimeoutError::Disconnected) => break,
                };
                match msg {
                    Msg::Shutdown => break,
                    Msg::Job(job, generation) => {
                        if generation != worker_generation.load(Ordering::SeqCst) {
                            let _ = etx.send(DownloadEvent::Finished {
                                track_id: job.track_id,
                                result: DownloadResult::Cancelled,
                            });
                            continue;
                        }
                        let tools = worker_tools.lock().unwrap_or_else(|p| p.into_inner()).clone();
                        let result = run_job(&job, &dest_dir, &tools, &worker_cancel, &worker_current, &etx);
                        let _ = etx.send(DownloadEvent::Finished {
                            track_id: job.track_id,
                            result,
                        });
                    }
                }
                if worker_cancel.load(Ordering::SeqCst) {
                    worker_cancel.store(false, Ordering::SeqCst);
                }
            })
            .map_err(|e| tracing::error!("could not start the music download worker: {e}"))
            .ok();

        DownloadQueue {
            tx,
            events: erx,
            generation,
            tools,
            cancel,
            current,
            join,
        }
    }

    pub fn enqueue(&self, job: DownloadJob) {
        // A new import after "Cancel" must run again, including jobs that are still queued.
        self.cancel.store(false, Ordering::SeqCst);
        let generation = self.generation.load(Ordering::SeqCst);
        if self.tx.send(Msg::Job(job, generation)).is_err() {
            tracing::warn!("music download queue is closed");
        }
    }

    /// Kill the running download and drop everything still queued for this generation.
    pub fn cancel_all(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.cancel.store(true, Ordering::SeqCst);
        tools::kill_running(&self.current);
    }

    pub fn set_tools(&self, new: Tools) {
        *self.tools.lock().unwrap_or_else(|p| p.into_inner()) = new;
    }

    pub fn poll(&self) -> Vec<DownloadEvent> {
        self.events.try_iter().collect()
    }

    pub fn shutdown(&mut self) {
        self.cancel.store(true, Ordering::SeqCst);
        tools::kill_running(&self.current);
        let _ = self.tx.send(Msg::Shutdown);
        if let Some(join) = self.join.take() {
            // The worker checks the cancel flag every 60 ms, so this does not hang the window.
            let _ = join.join();
        }
    }
}

impl Drop for DownloadQueue {
    fn drop(&mut self) {
        self.shutdown();
    }
}

// ---- Worker ------------------------------------------------------------------------------------

#[derive(Debug)]
struct Candidate {
    webpage_url: String,
    duration_secs: Option<f64>,
    title: String,
}

#[derive(Debug)]
enum AttemptError {
    NoMatch(String),
    DurationMismatch { found: f64, target: f64, limit: f64 },
    Failed(String),
    Cancelled,
}

fn run_job(
    job: &DownloadJob,
    dest_dir: &Path,
    tools: &Tools,
    cancel: &AtomicBool,
    current: &RunningChild,
    events: &Sender<DownloadEvent>,
) -> DownloadResult {
    if job.dest.is_file() && validate_audio_file(&job.dest).is_ok() {
        return DownloadResult::Ready(DownloadedTrack {
            path: job.dest.clone(),
            source: "cache".to_string(),
            source_url: String::new(),
            duration_secs: Some(job.duration_secs),
        });
    }
    let (Some(yt_dlp), Some(_ffmpeg)) = (tools.yt_dlp.as_deref(), tools.ffmpeg.as_deref()) else {
        return DownloadResult::Failed(
            tools
                .problem()
                .unwrap_or_else(|| "yt-dlp and ffmpeg are required to download music.".to_string()),
        );
    };
    if job.duration_secs > MAX_MUSIC_SECS {
        return DownloadResult::Failed(format!(
            "The track is longer than the {} minute limit for music playback.",
            (MAX_MUSIC_SECS / 60.0) as u32
        ));
    }
    if let Err(e) = fs::create_dir_all(dest_dir) {
        return DownloadResult::Failed(format!("Could not create the music cache: {e}"));
    }
    cleanup_track_files(dest_dir, &job.track_id);

    let query = job.query();
    if query.trim().is_empty() {
        return DownloadResult::Failed("The track has no title to search for.".to_string());
    }

    let attempts = [
        (Stage::SoundCloud, "scsearch1:", "", SOUNDCLOUD_TOLERANCE),
        (Stage::YouTube, "ytsearch1:", " official audio", YOUTUBE_TOLERANCE),
    ];
    let mut reasons: Vec<String> = Vec::new();
    for (stage, prefix, suffix, tolerance) in attempts {
        if cancel.load(Ordering::SeqCst) {
            return DownloadResult::Cancelled;
        }
        let _ = events.send(DownloadEvent::Started {
            track_id: job.track_id.clone(),
            stage,
        });
        let search = format!("{prefix}{query}{suffix}");
        match attempt(job, stage, &search, tolerance, dest_dir, yt_dlp, tools, cancel, current) {
            Ok(downloaded) => return DownloadResult::Ready(downloaded),
            Err(AttemptError::Cancelled) => return DownloadResult::Cancelled,
            Err(AttemptError::NoMatch(why)) => {
                reasons.push(format!("{}: {why}", stage.label()));
            }
            Err(AttemptError::DurationMismatch { found, target, limit }) => {
                reasons.push(format!(
                    "{}: the closest match is {:.0} s long, Spotify says {:.0} s (allowed difference {:.0} s)",
                    stage.label(),
                    found,
                    target,
                    limit
                ));
            }
            Err(AttemptError::Failed(why)) => {
                reasons.push(format!("{}: {why}", stage.label()));
            }
        }
    }
    let message = if reasons.is_empty() {
        "No source had this track.".to_string()
    } else {
        format!("No usable source. {}", reasons.join("; "))
    };
    cleanup_track_files(dest_dir, &job.track_id);
    DownloadResult::Failed(message)
}

#[allow(clippy::too_many_arguments)]
fn attempt(
    job: &DownloadJob,
    stage: Stage,
    search: &str,
    tolerance: f64,
    dest_dir: &Path,
    yt_dlp: &Path,
    tools: &Tools,
    cancel: &AtomicBool,
    current: &RunningChild,
) -> Result<DownloadedTrack, AttemptError> {
    let candidate = search_one(yt_dlp, search, cancel, current)
        .map_err(AttemptError::Failed)?
        .ok_or_else(|| AttemptError::NoMatch("nothing was found".to_string()))?;
    if let Some(found) = candidate.duration_secs {
        if job.duration_secs > 0.0 && (found - job.duration_secs).abs() > tolerance {
            return Err(AttemptError::DurationMismatch {
                found,
                target: job.duration_secs,
                limit: tolerance,
            });
        }
    } else if job.duration_secs > 0.0 {
        // A match without a duration cannot be checked for remixes; do not accept it blindly.
        return Err(AttemptError::NoMatch(
            "the result has no duration to compare".to_string(),
        ));
    }
    tracing::info!(
        "music: {} matched \"{}\" on {} ({} s)",
        job.title,
        candidate.title,
        stage.label(),
        candidate
            .duration_secs
            .map(|d| format!("{d:.0}"))
            .unwrap_or_else(|| "?".to_string())
    );

    let template = format!("{}.%(ext)s", job.dest.with_extension("").display());
    let mut args: Vec<OsString> = vec![
        "--ignore-config".into(),
        "--no-warnings".into(),
        "--no-playlist".into(),
        "--playlist-items".into(),
        "1".into(),
        "--no-progress".into(),
        "--newline".into(),
        "--retries".into(),
        "3".into(),
        "--fragment-retries".into(),
        "3".into(),
        "--socket-timeout".into(),
        "25".into(),
        "-f".into(),
        "bestaudio/best".into(),
        "-x".into(),
        "--audio-format".into(),
        "mp3".into(),
        "--audio-quality".into(),
        "0".into(),
        "--output".into(),
        template.into(),
    ];
    if let Some(dir) = tools.ffmpeg_dir() {
        args.push("--ffmpeg-location".into());
        args.push(dir.into_os_string());
    }
    args.push(candidate.webpage_url.clone().into());

    let result = tools::run_capture(yt_dlp, &args, DOWNLOAD_TIMEOUT, cancel, current).map_err(AttemptError::Failed)?;
    if result.cancelled {
        return Err(AttemptError::Cancelled);
    }
    if result.timed_out {
        return Err(AttemptError::Failed("the download timed out".to_string()));
    }
    if !result.success() {
        return Err(AttemptError::Failed(tail_or(&result, "yt-dlp failed")));
    }
    let path = find_output(dest_dir, &job.track_id).ok_or_else(|| {
        AttemptError::Failed(format!("yt-dlp finished but produced no file. {}", result.error_tail()))
    })?;
    validate_audio_file(&path)
        .map_err(|e| AttemptError::Failed(format!("the downloaded file is not playable: {e}")))?;
    Ok(DownloadedTrack {
        path,
        source: stage.label().to_string(),
        source_url: candidate.webpage_url,
        duration_secs: candidate.duration_secs,
    })
}

fn search_one(
    yt_dlp: &Path,
    search: &str,
    cancel: &AtomicBool,
    current: &RunningChild,
) -> Result<Option<Candidate>, String> {
    let args: Vec<OsString> = vec![
        "--ignore-config".into(),
        "--no-warnings".into(),
        "--no-playlist".into(),
        "--playlist-items".into(),
        "1".into(),
        "--skip-download".into(),
        "--dump-json".into(),
        search.into(),
    ];
    let result = tools::run_capture(yt_dlp, &args, SEARCH_TIMEOUT, cancel, current)?;
    if result.cancelled {
        return Ok(None);
    }
    if result.timed_out {
        return Err("the search timed out".to_string());
    }
    if !result.success() {
        return Err(tail_or(&result, "the search failed"));
    }
    for line in result.stdout.lines().map(str::trim).filter(|l| !l.is_empty()) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(line) else {
            continue;
        };
        if let Some(candidate) = candidate_from_json(&value) {
            return Ok(Some(candidate));
        }
        // Some extractors still wrap the search result in a playlist object.
        if let Some(entries) = value.get("entries").and_then(|e| e.as_array()) {
            for entry in entries {
                if let Some(candidate) = candidate_from_json(entry) {
                    return Ok(Some(candidate));
                }
            }
        }
    }
    Ok(None)
}

fn candidate_from_json(value: &serde_json::Value) -> Option<Candidate> {
    let webpage_url = value
        .get("webpage_url")
        .and_then(|v| v.as_str())
        .or_else(|| value.get("url").and_then(|v| v.as_str()))
        .filter(|u| u.starts_with("http"))?
        .to_string();
    let duration_secs = value
        .get("duration")
        .and_then(|v| v.as_f64())
        .filter(|d| d.is_finite() && *d > 0.0);
    let title = value.get("title").and_then(|v| v.as_str()).unwrap_or("").to_string();
    Some(Candidate {
        webpage_url,
        duration_secs,
        title,
    })
}

fn tail_or(result: &RunResult, fallback: &str) -> String {
    let tail = result.error_tail();
    if tail.is_empty() {
        fallback.to_string()
    } else {
        tail
    }
}

/// The file yt-dlp wrote: `<track id>.<ext>`, after post-processing normally `.mp3`.
fn find_output(dir: &Path, track_id: &str) -> Option<PathBuf> {
    let entries = fs::read_dir(dir).ok()?;
    let mut best: Option<PathBuf> = None;
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("");
        if stem != track_id {
            continue;
        }
        match path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_ascii_lowercase())
        {
            Some(ext) if ext == "mp3" => return Some(path),
            Some(ext) if ext == "part" => {}
            _ => best = Some(path),
        }
    }
    best
}

/// Remove partial/leftover files of one track (never anything outside the music cache).
fn cleanup_track_files(dir: &Path, track_id: &str) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        if path.file_stem().and_then(|s| s.to_str()) == Some(track_id) {
            let _ = fs::remove_file(&path);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn job() -> DownloadJob {
        DownloadJob {
            track_id: "abc".to_string(),
            title: "Come Together - Remastered 2009".to_string(),
            artist: "The Beatles".to_string(),
            duration_secs: 259.946,
            dest: PathBuf::from("music/abc.mp3"),
        }
    }

    #[test]
    fn query_uses_clean_title_and_artist() {
        assert_eq!(job().query(), "The Beatles - Come Together");
    }

    #[test]
    fn duration_rules_match_the_spec() {
        assert!((259.946_f64 - 250.0).abs() < SOUNDCLOUD_TOLERANCE);
        assert!((259.946_f64 - 280.0).abs() > SOUNDCLOUD_TOLERANCE);
        assert!((259.946_f64 - 275.0).abs() < YOUTUBE_TOLERANCE);
    }

    #[test]
    fn candidate_requires_an_http_url() {
        let value = serde_json::json!({"webpage_url": "https://soundcloud.com/a/b", "duration": 12.5, "title": "x"});
        let c = candidate_from_json(&value).unwrap();
        assert_eq!(c.duration_secs, Some(12.5));
        assert!(candidate_from_json(&serde_json::json!({"url": "notaurl"})).is_none());
    }

    #[test]
    fn find_output_prefers_mp3() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("id.webm"), b"x").unwrap();
        fs::write(dir.path().join("id.mp3"), b"ID3x").unwrap();
        let found = find_output(dir.path(), "id").unwrap();
        assert_eq!(found.extension().unwrap(), "mp3");
    }

    #[test]
    fn cleanup_removes_only_that_track() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("id.mp3"), b"x").unwrap();
        fs::write(dir.path().join("id.part"), b"x").unwrap();
        fs::write(dir.path().join("other.mp3"), b"x").unwrap();
        cleanup_track_files(dir.path(), "id");
        assert!(!dir.path().join("id.mp3").exists());
        assert!(!dir.path().join("id.part").exists());
        assert!(dir.path().join("other.mp3").exists());
    }

    #[test]
    fn an_idle_worker_stays_alive_for_later_jobs() {
        let dir = tempfile::tempdir().unwrap();
        let mut queue = start(dir.path().to_path_buf(), Tools::default());
        // Longer than the worker's receive timeout: it must not exit while idle.
        std::thread::sleep(Duration::from_millis(700));
        let track = MusicTrack::new("Late song", "Artist", "Album", 60.0, None);
        queue.enqueue(DownloadJob::from_track(&track, dir.path().join("late.mp3")));
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        let mut finished = false;
        while std::time::Instant::now() < deadline {
            for event in queue.poll() {
                if let DownloadEvent::Finished { result, .. } = event {
                    // yt-dlp is not configured in this test, so the job must fail with a
                    // message - the important part is that the late job was picked up at all.
                    assert!(matches!(result, DownloadResult::Failed(_)));
                    finished = true;
                }
            }
            if finished {
                break;
            }
            std::thread::sleep(Duration::from_millis(50));
        }
        queue.shutdown();
        assert!(
            finished,
            "the worker did not process a job enqueued after an idle period"
        );
    }
}
