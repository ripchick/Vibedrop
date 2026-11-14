//! External tools used by the Music section: `yt-dlp` (search + download) and `ffmpeg`
//! (audio extraction/conversion), plus a small cancellable process runner.
//!
//! Nothing is installed automatically. A path can be set in Settings > Music; otherwise the
//! program is looked up on `PATH` and in the usual package-manager directories. Both tools are
//! only ever started with an explicit argument list (no shell), so titles cannot inject commands.

use crate::settings::MusicSettings;
use std::ffi::OsString;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Where yt-dlp is downloaded (the releases page with the ready-made executables).
pub const YT_DLP_PAGE: &str = "https://github.com/yt-dlp/yt-dlp/releases";
/// Where FFmpeg is downloaded (the official download page with the Windows/macOS/Linux builds).
pub const FFMPEG_PAGE: &str = "https://ffmpeg.org/download.html";

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Tools {
    pub yt_dlp: Option<PathBuf>,
    pub ffmpeg: Option<PathBuf>,
    pub yt_dlp_version: Option<String>,
    pub ffmpeg_version: Option<String>,
}

impl Tools {
    /// Locate both programs and ask them for their version. Cheap enough to run when the user
    /// opens the Music page or changes the paths, but not on every frame.
    pub fn discover(settings: &MusicSettings) -> Tools {
        let yt_dlp = find_program(&settings.yt_dlp_path, &["yt-dlp", "yt-dlp.exe"]);
        let ffmpeg = find_program(&settings.ffmpeg_path, &["ffmpeg", "ffmpeg.exe"]);
        let yt_dlp_version = yt_dlp.as_deref().and_then(|p| version_of(p, &["--version"]));
        let ffmpeg_version = ffmpeg.as_deref().and_then(|p| version_of(p, &["-version"]));
        Tools {
            yt_dlp,
            ffmpeg,
            yt_dlp_version,
            ffmpeg_version,
        }
    }

    pub fn ready(&self) -> bool {
        self.yt_dlp.is_some() && self.ffmpeg.is_some()
    }

    /// One line for the UI: what is missing and how to fix it.
    pub fn problem(&self) -> Option<String> {
        match (self.yt_dlp.is_some(), self.ffmpeg.is_some()) {
            (true, true) => None,
            (false, true) => Some(
                "yt-dlp was not found. Install it (for example \"winget install yt-dlp.yt-dlp\" or \
                 \"pip install -U yt-dlp\") and set its path in Settings > Music if it is not on PATH."
                    .to_string(),
            ),
            (true, false) => Some(
                "ffmpeg was not found. Install it (for example \"winget install Gyan.FFmpeg\" or \
                 your package manager) and set its path in Settings > Music if it is not on PATH."
                    .to_string(),
            ),
            (false, false) => Some(
                "yt-dlp and ffmpeg were not found. Both are needed to download music: yt-dlp finds \
                 the audio and ffmpeg converts it to MP3. Install them and set their paths in \
                 Settings > Music if they are not on PATH."
                    .to_string(),
            ),
        }
    }

    /// Directory passed to `yt-dlp --ffmpeg-location`. The configured path may be the binary or
    /// the folder that contains it.
    pub fn ffmpeg_dir(&self) -> Option<PathBuf> {
        self.ffmpeg.as_ref().map(|p| {
            if p.is_dir() {
                p.clone()
            } else {
                p.parent().map(Path::to_path_buf).unwrap_or_else(|| p.clone())
            }
        })
    }
}

fn version_of(program: &Path, args: &[&str]) -> Option<String> {
    let cancel = AtomicBool::new(false);
    let current = Mutex::new(None);
    let result = run_capture(
        program,
        &args.iter().map(OsString::from).collect::<Vec<_>>(),
        Duration::from_secs(6),
        &cancel,
        &current,
    )
    .ok()?;
    if !result.success() {
        return None;
    }
    result
        .stdout
        .lines()
        .chain(result.stderr.lines())
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(|line| line.chars().take(120).collect())
}

/// Returns true when `name` exists as a file (or as `name.exe` when `name` has no extension).
fn is_program(path: &Path) -> bool {
    if !path.is_file() {
        return false;
    }
    is_program_file(path)
}

/// Whether an existing file may be executed.
///
/// On Windows only known executable extensions are accepted; on Unix-like systems every file
/// with an executable bit is a program, so the name itself does not matter.
#[cfg(windows)]
fn is_program_file(path: &Path) -> bool {
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    matches!(ext.to_ascii_lowercase().as_str(), "exe" | "cmd" | "bat" | "com" | "ps1")
}

#[cfg(not(windows))]
fn is_program_file(_path: &Path) -> bool {
    true
}

fn in_dir(dir: &Path, names: &[&str]) -> Option<PathBuf> {
    for name in names {
        let candidate = dir.join(name);
        if is_program(&candidate) {
            return Some(candidate);
        }
    }
    // An unpacked archive usually keeps the binaries in `bin/`.
    for name in names {
        let candidate = dir.join("bin").join(name);
        if is_program(&candidate) {
            return Some(candidate);
        }
    }
    None
}

fn common_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.to_path_buf());
            dirs.push(parent.join("tools"));
        }
    }
    if let Some(local) = std::env::var_os("LOCALAPPDATA") {
        let local = PathBuf::from(local);
        dirs.push(local.join("Programs").join("yt-dlp"));
        dirs.push(local.join("Microsoft").join("WinGet").join("Links"));
    }
    if let Some(profile) = std::env::var_os("USERPROFILE") {
        let profile = PathBuf::from(profile);
        dirs.push(profile.join("scoop").join("shims"));
        dirs.push(profile.join("AppData").join("Roaming").join("Python").join("Scripts"));
    }
    if let Some(program_data) = std::env::var_os("PROGRAMDATA") {
        dirs.push(PathBuf::from(program_data).join("chocolatey").join("bin"));
    }
    for dir in ["/usr/local/bin", "/usr/bin", "/opt/homebrew/bin"] {
        dirs.push(PathBuf::from(dir));
    }
    dirs
}

/// Find a program: an explicit file or folder first, then `PATH`, then common install folders.
pub fn find_program(explicit: &str, names: &[&str]) -> Option<PathBuf> {
    let explicit = explicit.trim();
    if !explicit.is_empty() {
        let path = PathBuf::from(explicit);
        if is_program(&path) {
            return Some(path);
        }
        if path.is_dir() {
            if let Some(found) = in_dir(&path, names) {
                return Some(found);
            }
        }
        // A wrong explicit path is a user error; do not silently fall back to a different copy.
        return None;
    }
    if let Some(paths) = std::env::var_os("PATH") {
        for dir in std::env::split_paths(&paths) {
            if let Some(found) = in_dir(&dir, names) {
                return Some(found);
            }
        }
    }
    for dir in common_dirs() {
        if let Some(found) = in_dir(&dir, names) {
            return Some(found);
        }
    }
    None
}

// ---- Process runner ----------------------------------------------------------------------------

#[derive(Debug, Clone, Default)]
pub struct RunResult {
    pub status: Option<i32>,
    pub stdout: String,
    pub stderr: String,
    pub timed_out: bool,
    pub cancelled: bool,
}

impl RunResult {
    pub fn success(&self) -> bool {
        self.status == Some(0) && !self.timed_out && !self.cancelled
    }

    /// Last few meaningful lines, for an error message. Lines that mention an error or ffmpeg win
    /// over the ordinary download progress, which is often what came last.
    pub fn error_tail(&self) -> String {
        let all: Vec<&str> = self
            .stderr
            .lines()
            .chain(self.stdout.lines())
            .map(str::trim)
            .filter(|l| !l.is_empty())
            .collect();
        let interesting: Vec<&str> = all
            .iter()
            .copied()
            .filter(|l| {
                let lower = l.to_ascii_lowercase();
                lower.contains("error")
                    || lower.contains("ffmpeg")
                    || lower.contains("not found")
                    || lower.contains("unable")
                    || lower.contains("failed")
            })
            .collect();
        let mut lines = if interesting.is_empty() { all } else { interesting };
        if lines.len() > 4 {
            lines = lines.split_off(lines.len() - 4);
        }
        let mut text = lines.join(" ");
        if text.len() > 400 {
            text.truncate(400);
            text.push('…');
        }
        text
    }
}

/// Run a program with captured stdout/stderr, a timeout and cooperative cancellation.
///
/// `current` lets another thread kill the running child (the Music page's "Cancel" button). The
/// child is never run through a shell, and stdin is closed so a tool can never wait for input.
pub fn run_capture(
    program: &Path,
    args: &[OsString],
    timeout: Duration,
    cancel: &AtomicBool,
    current: &Mutex<Option<Child>>,
) -> Result<RunResult, String> {
    let mut command = Command::new(program);
    command
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        command.creation_flags(CREATE_NO_WINDOW);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start {}: {e}", program.display()))?;

    let mut stdout = child.stdout.take();
    let mut stderr = child.stderr.take();
    let out_thread = stdout.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });
    let err_thread = stderr.take().map(|mut pipe| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = pipe.read_to_end(&mut buf);
            buf
        })
    });

    {
        let mut slot = current.lock().unwrap_or_else(|p| p.into_inner());
        *slot = Some(child);
    }

    let deadline = Instant::now() + timeout;
    let mut status: Option<i32> = None;
    let mut timed_out = false;
    let mut cancelled = false;
    loop {
        let mut finished = false;
        {
            let mut slot = current.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(child) = slot.as_mut() {
                match child.try_wait() {
                    Ok(Some(exit)) => {
                        status = exit.code();
                        finished = true;
                    }
                    Ok(None) => {}
                    Err(e) => {
                        let _ = child.kill();
                        let _ = child.wait();
                        *slot = None;
                        return Err(format!("Could not wait for {}: {e}", program.display()));
                    }
                }
            }
            if finished {
                *slot = None;
            }
        }
        if finished {
            break;
        }
        if cancel.load(Ordering::SeqCst) {
            cancelled = true;
        } else if Instant::now() >= deadline {
            timed_out = true;
        }
        if cancelled || timed_out {
            let mut slot = current.lock().unwrap_or_else(|p| p.into_inner());
            if let Some(child) = slot.as_mut() {
                let _ = child.kill();
                let _ = child.wait();
            }
            *slot = None;
            break;
        }
        std::thread::sleep(Duration::from_millis(60));
    }

    let stdout = out_thread
        .and_then(|t| t.join().ok())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    let stderr = err_thread
        .and_then(|t| t.join().ok())
        .map(|b| String::from_utf8_lossy(&b).into_owned())
        .unwrap_or_default();
    Ok(RunResult {
        status,
        stdout,
        stderr,
        timed_out,
        cancelled,
    })
}

/// Shared handle used by the download worker and the Cancel button.
pub type RunningChild = Arc<Mutex<Option<Child>>>;

pub fn new_running_child() -> RunningChild {
    Arc::new(Mutex::new(None))
}

pub fn kill_running(child: &RunningChild) {
    let mut slot = child.lock().unwrap_or_else(|p| p.into_inner());
    if let Some(mut c) = slot.take() {
        let _ = c.kill();
        let _ = c.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn explicit_missing_path_is_not_silently_replaced() {
        assert!(find_program("C:\\definitely-not-here\\yt-dlp.exe", &["yt-dlp.exe"]).is_none());
    }

    #[test]
    fn version_and_run_report_failure_without_panicking() {
        let cancel = AtomicBool::new(false);
        let current = Mutex::new(None);
        // A command that is guaranteed to fail on both Windows and Unix.
        #[cfg(windows)]
        let (program, args) = (
            PathBuf::from("cmd.exe"),
            vec![OsString::from("/c"), OsString::from("exit 1")],
        );
        #[cfg(not(windows))]
        let (program, args) = (
            PathBuf::from("/bin/sh"),
            vec![OsString::from("-c"), OsString::from("exit 1")],
        );
        let result = run_capture(&program, &args, Duration::from_secs(5), &cancel, &current)
            .expect("the program can be started");
        assert!(!result.success());
        assert!(!result.timed_out);
    }

    #[test]
    fn a_cancelled_process_is_killed() {
        let cancel = AtomicBool::new(false);
        let current = new_running_child();
        #[cfg(windows)]
        let (program, args) = (
            PathBuf::from("cmd.exe"),
            vec![OsString::from("/c"), OsString::from("ping -n 6 127.0.0.1 >nul")],
        );
        #[cfg(not(windows))]
        let (program, args) = (
            PathBuf::from("/bin/sh"),
            vec![OsString::from("-c"), OsString::from("sleep 5")],
        );
        let flag = Arc::new(cancel);
        let watcher = {
            let flag = Arc::clone(&flag);
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(200));
                flag.store(true, Ordering::SeqCst);
            })
        };
        let result =
            run_capture(&program, &args, Duration::from_secs(30), &flag, &current).expect("the program can be started");
        let _ = watcher.join();
        assert!(result.cancelled, "the child must be reported as cancelled");
        assert!(!result.timed_out);
    }
}
