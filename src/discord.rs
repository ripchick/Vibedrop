//! Discord Rich Presence over the local IPC endpoint (named pipe / Unix socket).
//!
//! Shows "VibeDrop" in Discord's activity list and, while a sound plays, what is
//! currently playing with the elapsed time (like Spotify). If Discord is not running,
//! the worker simply retries quietly.

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
    /// The picture shown next to the activity. Discord accepts a direct image address here, so
    /// the cover of the album that is playing can be shown.
    image: Option<String>,
    /// The tooltip of that picture.
    image_text: Option<String>,
}

pub struct DiscordPresence {
    tx: Sender<Presence>,
    status: Arc<Mutex<DiscordStatus>>,
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
        self.set_full(details, state, started, None, None);
    }

    /// Like [`DiscordPresence::set`], but with a picture (a direct image address).
    pub fn set_with_image(
        &self,
        details: Option<String>,
        state: Option<String>,
        started: Option<i64>,
        image: Option<String>,
        image_text: Option<String>,
    ) {
        self.set_full(details, state, started, image, image_text);
    }

    fn set_full(
        &self,
        details: Option<String>,
        state: Option<String>,
        started: Option<i64>,
        image: Option<String>,
        image_text: Option<String>,
    ) {
        let _ = self.tx.try_send(Presence {
            details,
            state,
            started,
            image,
            image_text,
        });
    }

    pub fn clear(&self) {
        self.set(None, None, None);
    }
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

fn worker(rx: Receiver<Presence>, status: Arc<Mutex<DiscordStatus>>) {
    let pid = std::process::id();
    let mut last: Option<Presence> = None;
    loop {
        let mut stream = match connect() {
            Some(s) => s,
            None => {
                *status.lock().unwrap_or_else(|e| e.into_inner()) = DiscordStatus::Unavailable;
                std::thread::sleep(Duration::from_secs(10));
                continue;
            }
        };
        *status.lock().unwrap_or_else(|e| e.into_inner()) = DiscordStatus::Connecting;
        if handshake(&mut stream).is_err() {
            std::thread::sleep(Duration::from_secs(5));
            continue;
        }
        *status.lock().unwrap_or_else(|e| e.into_inner()) = DiscordStatus::Connected;
        // Keep the activity fresh: the app pushes updates on change, the worker also
        // re-sends every 15 s so the elapsed readout stays alive.
        let mut last_sent = std::time::Instant::now() - Duration::from_secs(15);
        let mut last_sent_presence: Option<Presence> = None;
        loop {
            while let Ok(p) = rx.try_recv() {
                last = Some(p);
            }
            let now = std::time::Instant::now();
            let changed = match (&last, &last_sent_presence) {
                (Some(a), Some(b)) => {
                    a.details != b.details || a.state != b.state || a.started != b.started || a.image != b.image
                }
                (Some(_), None) => true,
                (None, Some(_)) => true,
                (None, None) => false,
            };
            if changed || now.duration_since(last_sent) >= Duration::from_secs(15) {
                let presence = last.clone();
                if send_activity(&mut stream, pid, presence.as_ref()).is_err() {
                    break;
                }
                // Discord answers every command. Nobody reads those answers, so they pile up in
                // the pipe until it is full - and then writing blocks for good, which is why the
                // elapsed time used to freeze. Reading them away fixes that.
                drain_answers(&mut stream);
                last_sent = now;
                last_sent_presence = last.clone();
            }
            std::thread::sleep(Duration::from_millis(400));
        }
    }
}

/// Throw away whatever Discord has sent so far without blocking.
#[cfg(windows)]
fn drain_answers(stream: &mut Transport) {
    use std::os::windows::io::AsRawHandle;
    use winapi::um::namedpipeapi::PeekNamedPipe;
    use winapi::um::winnt::HANDLE;
    match stream {
        Transport::Windows(file) => {
            let handle = file.as_raw_handle() as HANDLE;
            let mut available: u32 = 0;
            let ok = unsafe {
                PeekNamedPipe(
                    handle,
                    std::ptr::null_mut(),
                    0,
                    std::ptr::null_mut(),
                    &mut available,
                    std::ptr::null_mut(),
                )
            };
            if ok != 0 && available > 0 {
                let mut buffer = vec![0u8; available as usize];
                use std::io::Read;
                let _ = file.read(&mut buffer);
            }
        }
    }
}

/// Throw away whatever Discord has sent so far without blocking.
#[cfg(not(windows))]
fn drain_answers(stream: &mut Transport) {
    use std::io::Read;
    match stream {
        Transport::Unix(socket) => {
            let _ = socket.set_read_timeout(Some(Duration::from_millis(1)));
            let mut buffer = [0u8; 4096];
            loop {
                match socket.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(_) => continue,
                    Err(_) => break,
                }
            }
            let _ = socket.set_read_timeout(Some(Duration::from_secs(2)));
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

/// Discord's local RPC handshake: `{"v":1,"client_id":"..."}` as opcode 0.
fn handshake(stream: &mut Transport) -> std::io::Result<()> {
    let payload = format!("{{\"v\":1,\"client_id\":\"{CLIENT_ID}\"}}");
    write_frame(stream, 0, &payload)?;
    // READY frame comes right after the handshake.
    let _ = read_frame(stream);
    Ok(())
}

fn send_activity(stream: &mut Transport, pid: u32, presence: Option<&Presence>) -> std::io::Result<()> {
    let payload = activity_payload(pid, presence);
    write_frame(stream, 1, &payload)
}

/// The JSON of one SET_ACTIVITY command.
fn activity_payload(pid: u32, presence: Option<&Presence>) -> String {
    let activity = match presence {
        Some(p) if p.details.is_some() => {
            let details = p.details.as_deref().unwrap_or_default();
            let state = p.state.as_deref().unwrap_or_default();
            let timestamps = match p.started {
                Some(s) => format!(",\"timestamps\":{{\"start\":{s}}}"),
                None => String::new(),
            };
            // The picture: the cover of the album that is playing (a direct address) or the
            // application's own logo.
            let assets = match p.image.as_deref() {
                Some(url) if !url.is_empty() => {
                    let text = match p.image_text.as_deref() {
                        Some(t) if !t.is_empty() => format!(",\"large_text\":{}", json_str(t)),
                        _ => String::new(),
                    };
                    format!("\"assets\":{{\"large_image\":{}{text}}}", json_str(url))
                }
                _ => "\"assets\":{\"large_image\":\"logo\"}".to_string(),
            };
            format!(
                "{{\"name\":\"{ACTIVITY_NAME}\",\"type\":2,\"application_id\":\"{CLIENT_ID}\",\
                 \"details\":{},\"state\":{},{assets}{timestamps},\
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
    format!(
        "{{\"cmd\":\"SET_ACTIVITY\",\"nonce\":\"{}\",\"args\":{{\"pid\":{pid},\"activity\":{activity}}}}}",
        nonce()
    )
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
            // SAFETY: the handle was just created successfully; we take ownership.
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

#[cfg(test)]
mod tests {
    use super::*;

    fn presence(image: Option<&str>) -> Presence {
        Presence {
            details: Some("Кино - Спокойная ночь".to_string()),
            state: Some("0:03 / 3:34".to_string()),
            started: Some(1_700_000_000),
            image: image.map(str::to_string),
            image_text: Some("Спокойная ночь".to_string()),
        }
    }

    #[test]
    fn the_cover_is_sent_as_the_picture() {
        let url = "https://image-cdn-fa.spotifycdn.com/image/ab67616d0000b273";
        let payload = activity_payload(42, Some(&presence(Some(url))));
        assert!(payload.contains("\"cmd\":\"SET_ACTIVITY\""), "{payload}");
        assert!(payload.contains(&format!("\"large_image\":\"{url}\"")), "{payload}");
        assert!(payload.contains("\"large_text\":\"Спокойная ночь\""), "{payload}");
        assert!(payload.contains("\"start\":1700000000"), "{payload}");
        // The payload has to be valid JSON.
        let value: serde_json::Value = serde_json::from_str(&payload).expect("valid JSON");
        let activity = &value["args"]["activity"];
        assert_eq!(activity["type"], 2);
        assert_eq!(activity["details"], "Кино - Спокойная ночь");
    }

    #[test]
    fn without_a_cover_the_logo_is_shown() {
        let payload = activity_payload(42, Some(&presence(None)));
        assert!(payload.contains("\"large_image\":\"logo\""), "{payload}");
        let value: serde_json::Value = serde_json::from_str(&payload).expect("valid JSON");
        assert_eq!(value["args"]["activity"]["assets"]["large_image"], "logo");
    }

    #[test]
    fn without_a_track_only_the_application_is_shown() {
        let payload = activity_payload(42, None);
        let value: serde_json::Value = serde_json::from_str(&payload).expect("valid JSON");
        assert_eq!(value["args"]["activity"]["type"], 0);
        assert_eq!(value["args"]["activity"]["assets"]["large_image"], "logo");
    }
}
