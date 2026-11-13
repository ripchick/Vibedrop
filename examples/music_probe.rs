//! Live diagnostic for the Music section: read a Spotify link and download its first track
//! through exactly the same cascade the app uses.
//!
//! ```text
//! cargo run --example music_probe -- https://open.spotify.com/album/0ETFjACtuP2ADo6LFhL6HN
//! cargo run --example music_probe -- https://open.spotify.com/playlist/37i9dQZF1DXcBWIGoYBM5M
//! ```
//!
//! `yt-dlp` is looked up on PATH unless `YTDLP_PATH` is set; `ffmpeg` on PATH unless
//! `FFMPEG_DIR` points at the folder that contains it. The MP3 is written to
//! `target/music-probe/`, never into the user's data folder.

use std::path::PathBuf;
use std::time::{Duration, Instant};
use vibedrop::music::download::{self, DownloadEvent, DownloadJob, DownloadResult};
use vibedrop::music::spotify;
use vibedrop::music::tools::Tools;
use vibedrop::music::MusicTrack;

fn main() -> Result<(), String> {
    let url = std::env::args()
        .nth(1)
        .ok_or_else(|| "usage: music_probe <spotify track/album/playlist URL>".to_string())?;
    let link = spotify::parse_link(&url)?;
    println!("link: {:?} {}", link.kind, link.id);
    let entity = spotify::fetch(&link, None)?;
    println!(
        "fetched from {}: {} — {} ({} tracks, cover: {})",
        entity.source,
        entity.artist,
        entity.title,
        entity.tracks.len(),
        entity.cover_url.as_deref().unwrap_or("none")
    );
    for (i, t) in entity.tracks.iter().take(5).enumerate() {
        println!("  {}. {} — {} ({:.0}s)", i + 1, t.artist, t.title, t.duration_secs);
    }

    let yt_dlp = std::env::var_os("YTDLP_PATH").map(PathBuf::from);
    let ffmpeg = std::env::var_os("FFMPEG_DIR").map(PathBuf::from);
    if yt_dlp.is_none() || ffmpeg.is_none() {
        println!("YTDLP_PATH / FFMPEG_DIR not set; stopping after metadata.");
        return Ok(());
    }
    let tools = Tools {
        yt_dlp,
        ffmpeg,
        ..Tools::default()
    };

    let dest = PathBuf::from("target").join("music-probe");
    std::fs::create_dir_all(&dest).map_err(|e| format!("cannot create {}: {e}", dest.display()))?;
    let first = &entity.tracks[0];
    let track = MusicTrack::new(
        first.title.clone(),
        first.artist.clone(),
        entity.title.clone(),
        first.duration_secs,
        first.spotify_id.clone(),
    );
    let job = DownloadJob::from_track(&track, dest.join(format!("{}.mp3", track.id)));
    let mut queue = download::start(dest, tools);
    queue.enqueue(job);
    let deadline = Instant::now() + Duration::from_secs(15 * 60);
    loop {
        for event in queue.poll() {
            match event {
                DownloadEvent::Started { stage, .. } => println!("searching {}", stage.label()),
                DownloadEvent::Finished { result, .. } => {
                    let outcome = match result {
                        DownloadResult::Ready(file) => {
                            println!(
                                "ready: {} (source: {}, {:.0}s)",
                                file.path.display(),
                                file.source,
                                file.duration_secs.unwrap_or(0.0)
                            );
                            Ok(())
                        }
                        DownloadResult::Cancelled => {
                            println!("cancelled");
                            Ok(())
                        }
                        DownloadResult::Failed(error) => Err(error),
                    };
                    queue.shutdown();
                    return outcome;
                }
            }
        }
        if Instant::now() > deadline {
            queue.shutdown();
            return Err("timed out".to_string());
        }
        std::thread::sleep(Duration::from_millis(100));
    }
}
