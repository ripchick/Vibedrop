//! Discord Rich Presence over the local IPC endpoint (named pipe / Unix socket).
//!
//! Shows "VibeDrop" in Discord and, while a sound plays, what is currently playing
//! with the elapsed time (like Spotify). If Discord is not running, the worker just
//! retries quietly.

use crossbeam_channel::{Receiver, Sender};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};
use std::time::Duration;

pub const CLIENT_ID: &str = "1569429358538526820";
const ACTIVITY_NAME: &str = "VibeDrop";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DiscordStatus {
    Unavailable,
    Connecting,
    Connected,
}

#[derive(Clone)]
struct Presence {
    details: Option<String>,
    state: Option<String>,
    started: Option<i64>,
}

pub struct DiscordPresence {
    tx: Sender<Presence>,
    status: Arc<Mutex<DiscordStatus>>,
}

enum Transport {
    #[cfg(windows)]
    Windows(std::fs::File),
    #[cfg(not(windows))]
    Unix(std::os::unix::net::UnixStream),
}

impl std::io::Read for Transport {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(windows)]
            Transport::Windows(f) => f.read(buf),
            #[cfg(not(windows))]
            Transport::Unix(s) => s.read(buf),
        }
    }
}

impl std::io::Write for Transport {
    fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
        match self {
            #[cfg(windows)]
            Transport::Windows(f) => f.write(buf),
            #[cfg(not(windows))]
            Transport::Unix(s) => s.write(buf),
        }
    }
    fn flush(&mut self) -> std::io::Result<()> {
        match self {
            #[cfg(windows)]
            Transport::Windows(f) => f.flush(),
            #[cfg(not(windows))]
            Transport::Unix(s) => s.flush(),
        }
    }
}

impl DiscordPresence {
    pub fn start() -> Self {
        let (tx, rx) = crossbeam_channel::unbounded::<Presence>();
        let status = Arc::new(Mutex::new(DiscordStatus::Connecting));
        let thread_status = Arc::clone(&status);
        std::thread::spawn(move || {
            worker(rx, thread_status);
        });
        Self { tx, status }
    }

    pub fn status(&self) -> DiscordStatus {
        *self.status.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Non-blocking; drops the update if the worker is busy.
    pub fn set(&self, details: Option<String>, state: Option<String>, started: Option<i64>) {
        let _ = self.tx.try_send(Presence {
            details,
            state,
            started,
        });
    }

    pub fn clear(&self) {
        self.set(None, None, None);
    }
}

fn worker(rx: Receiver<Presence>, status: Arc<Mutex<DiscordStatus>>) {
    let pid = std::process::id();
    let mut last: Option<Presence> = None;
    loop {
        let mut stream = match connect() {
            Some(s) => s,
            None => {
                *status.lock().unwrap_or_else(|e| e.into_inner()) = DiscordStatus::Unavailable;
                std::thread::sleep(Duration::from_secs(15));
                continue;
            }
        };
        *status.lock().unwrap_or_else(|e| e.into_inner()) = DiscordStatus::Connecting;
        match handshake(&mut stream, pid) {
            Ok(()) => {
                *status.lock().unwrap_or_else(|e| e.into_inner()) = DiscordStatus::Connected;
                // Keep the activity fresh; the app pushes updates on change, the worker
                // also re-sends every 15 s so the elapsed readout stays alive.
                let mut last_sent = std::time::Instant::now() - Duration::from_secs(15);
                let mut last_sent_presence: Option<Presence> = None;
                while let Ok(p) = rx.recv_timeout(Duration::from_millis(500)) {
                    last = Some(p);
                }
                loop {
                    while let Ok(p) = rx.try_recv() {
                        last = Some(p);
                    }
                    let now = std::time::Instant::now();
                    let changed = match (&last, &last_sent_presence) {
                        (Some(a), Some(b)) => a.details != b.details || a.state != b.state || a.started != b.started,
                        (Some(_), None) => true,
                        (None, Some(_)) => true,
                        (None, None) => false,
                    };
                    if changed || now.duration_since(last_sent) >= Duration::from_secs(15) {
                        let presence = last.clone();
                        if send_activity(&mut stream, pid, presence.as_ref()).is_err() {
                            break;
                        }
                        last_sent = now;
                        last_sent_presence = last.clone();
                    }
                    std::thread::sleep(Duration::from_millis(500));
                }
            }
            Err(_) => {
                std::thread::sleep(Duration::from_secs(10));
            }
        }
    }
}

fn nonce() -> String {
    uuid::Uuid::new_v4().to_string()
}

fn write_frame(stream: &mut impl Write, opcode: u32, payload: &str) -> std::io::Result<()> {
    let len = payload.len() as u32;
    stream.write_all(&opcode.to_le_bytes())?;
    stream.write_all(&len.to_le_bytes())?;
    stream.write_all(payload.as_bytes())?;
    stream.flush()
}

fn read_frame(stream: &mut impl Read) -> std::io::Result<(u32, Vec<u8>)> {
    let mut header = [0u8; 8];
    stream.read_exact(&mut header)?;
    let opcode = u32::from_le_bytes(header[0..4].try_into().unwrap());
    let len = u32::from_le_bytes(header[4..8].try_into().unwrap()) as usize;
    let mut payload = vec![0u8; len];
    stream.read_exact(&mut payload)?;
    Ok((opcode, payload))
}

fn handshake(stream: &mut Transport, _pid: u32) -> std::io::Result<()> {
    let payload = format!(
        "{{\"cmd\":\"IDENTIFY\",\"nonce\":\"{}\",\"args\":{{\"client_id\":\"{CLIENT_ID}\"}}}}",
        nonce()
    );
    write_frame(stream, 0, &payload)?;
    // Best effort: read the READY frame (Discord replies immediately).
    let _ = read_frame(stream);
    Ok(())
}

fn send_activity(stream: &mut Transport, pid: u32, presence: Option<&Presence>) -> std::io::Result<()> {
    let activity = match presence {
        Some(p) if p.details.is_some() => {
            let details = p.details.as_deref().unwrap_or_default();
            let state = p.state.as_deref().unwrap_or_default();
            let started = p.started.map(|s| s.to_string());
            let timestamps = match &started {
                Some(s) => format!(",\"timestamps\":{{\"start\":{s}}}"),
                None => String::new(),
            };
            format!(
                "{{\"name\":\"{ACTIVITY_NAME}\",\"type\":2,\"application_id\":\"{CLIENT_ID}\",\
                 \"details\":{},\"state\":{},\"assets\":{{\"large_image\":\"logo\"}}{timestamps},\
                 \"instance\":true}}",
                json_str(details),
                json_str(state),
            )
        }
        _ => format!(
            "{{\"name\":\"{ACTIVITY_NAME}\",\"type\":0,\"application_id\":\"{CLIENT_ID}\",\
             \"assets\":{{\"large_image\":\"logo\"}},\"instance\":true}}"
        ),
    };
    let payload = format!(
        "{{\"cmd\":\"SET_ACTIVITY\",\"nonce\":\"{}\",\"args\":{{\"pid\":{pid},\"activity\":{activity}}}}}",
        nonce()
    );
    write_frame(stream, 1, &payload)
}

fn json_str(s: &str) -> String {
    serde_json::to_string(s).unwrap_or_else(|_| "\"\"".to_string())
}

#[cfg(windows)]
fn connect() -> Option<Transport> {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::io::FromRawHandle;
    use winapi::um::fileapi::{CreateFileW, OPEN_EXISTING};
    use winapi::um::handleapi::INVALID_HANDLE_VALUE;
    use winapi::um::winnt::{FILE_SHARE_READ, FILE_SHARE_WRITE, GENERIC_READ, GENERIC_WRITE};
    for i in 0..10 {
        let path = format!(r"\\.\pipe\discord-ipc-{i}");
        let wide: Vec<u16> = OsString::from(path).encode_wide().chain(std::iter::once(0)).collect();
        let handle = unsafe {
            CreateFileW(
                wide.as_ptr(),
                GENERIC_READ | GENERIC_WRITE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
                std::ptr::null_mut(),
                OPEN_EXISTING,
                0,
                std::ptr::null_mut(),
            )
        };
        if handle != INVALID_HANDLE_VALUE {
            // SAFETY: FromRawHandle takes ownership; we just checked validity.
            let file = unsafe { std::fs::File::from_raw_handle(handle as _) };
            return Some(Transport::Windows(file));
        }
    }
    None
}

#[cfg(not(windows))]
fn connect() -> Option<Transport> {
    use std::os::unix::net::UnixStream;
    let mut candidates: Vec<std::path::PathBuf> = Vec::new();
    if let Ok(dir) = std::env::var("XDG_RUNTIME_DIR") {
        if !dir.is_empty() {
            candidates.push(std::path::PathBuf::from(dir));
        }
    }
    candidates.push(std::path::PathBuf::from("/tmp"));
    for dir in candidates {
        for i in 0..10 {
            if let Ok(stream) = UnixStream::connect(dir.join(format!("discord-ipc-{i}"))) {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let _ = stream.set_write_timeout(Some(Duration::from_secs(5)));
                return Some(Transport::Unix(stream));
            }
        }
    }
    None
}
