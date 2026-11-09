//! Local-network sound sharing between VibeDrop instances ("borrow a sound from a friend").
//!
//! Peers discover each other with a UDP broadcast ping every few seconds on the LAN.
//! A borrowing instance sends a REQUEST; the owner (if sharing is enabled in Settings)
//! answers by connecting over TCP and streaming the file. Nothing leaves the local
//! network and there is no central server.

use crossbeam_channel::{Receiver, Sender};
use serde::{Deserialize, Serialize};
use socket2::{Domain, Protocol, Socket, Type};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::net::{SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::path::PathBuf;
use std::time::Duration;

const DISCOVERY_PORT: u16 = 47821;
const HELLO_SECS: u64 = 5;
const PEER_TTL_SECS: u64 = 20;
const FILE_CAP: u64 = 100 * 1024 * 1024;

#[derive(Clone, Debug)]
pub struct PeerInfo {
    pub id: String,
    pub name: String,
    pub now_playing: Option<String>,
    pub addr: SocketAddr,
    pub last_seen: u64,
}

#[derive(Clone)]
pub enum ShareCmd {
    SetNowPlaying(Option<String>),
    SetAllow(bool),
    SetName(String),
    Request {
        addr: SocketAddr,
        sound: String,
    },
    /// Answer to an incoming request (None = deny).
    Respond {
        addr: SocketAddr,
        file: Option<PathBuf>,
    },
}

pub enum ShareMsg {
    Peers(Vec<PeerInfo>),
    IncomingRequest { addr: SocketAddr, sound: String },
    ReceivedFile { name: String, path: PathBuf },
    Note(String),
}

pub struct ShareHandle {
    cmd_tx: Sender<ShareCmd>,
    rx: Receiver<ShareMsg>,
}

impl ShareHandle {
    pub fn set_now_playing(&self, name: Option<String>) {
        let _ = self.cmd_tx.try_send(ShareCmd::SetNowPlaying(name));
    }
    pub fn set_allow(&self, allow: bool) {
        let _ = self.cmd_tx.try_send(ShareCmd::SetAllow(allow));
    }
    pub fn set_name(&self, name: String) {
        let _ = self.cmd_tx.try_send(ShareCmd::SetName(name));
    }
    pub fn request(&self, addr: SocketAddr, sound: String) {
        let _ = self.cmd_tx.try_send(ShareCmd::Request { addr, sound });
    }
    pub fn respond(&self, addr: SocketAddr, file: Option<PathBuf>) {
        let _ = self.cmd_tx.try_send(ShareCmd::Respond { addr, file });
    }
    pub fn poll(&self) -> Option<ShareMsg> {
        self.rx.try_recv().ok()
    }
}

fn unix_now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Serialize, Deserialize)]
struct Packet {
    t: String,
    id: String,
    name: String,
    playing: Option<String>,
    sound: Option<String>,
    port: Option<u16>,
}

pub fn start(display_name: String, allow: bool, sounds_dir: PathBuf) -> ShareHandle {
    let (cmd_tx, cmd_rx) = crossbeam_channel::unbounded::<ShareCmd>();
    let (msg_tx, msg_rx) = crossbeam_channel::unbounded::<ShareMsg>();
    let id = uuid::Uuid::new_v4().to_string();
    std::thread::spawn(move || {
        worker(id, display_name, allow, sounds_dir, cmd_rx, msg_tx);
    });
    ShareHandle { cmd_tx, rx: msg_rx }
}

fn worker(
    id: String,
    mut display_name: String,
    mut allow: bool,
    sounds_dir: PathBuf,
    cmd_rx: Receiver<ShareCmd>,
    msg_tx: Sender<ShareMsg>,
) {
    let socket = match udp_socket() {
        Ok(s) => s,
        Err(e) => {
            let _ = msg_tx.send(ShareMsg::Note(format!("Local sharing unavailable: {e}")));
            return;
        }
    };
    let mut now_playing: Option<String> = None;
    let mut peers: BTreeMap<String, PeerInfo> = BTreeMap::new();
    let mut last_hello = std::time::Instant::now() - Duration::from_secs(HELLO_SECS);
    // Pending borrow: (peer addr, sound name, listener, deadline) waiting for the TCP push.
    let mut pending: Option<(SocketAddr, String, TcpListener, std::time::Instant)> = None;
    let broadcast = SocketAddr::from(([255, 255, 255, 255], DISCOVERY_PORT));
    let mut buf = [0u8; 8192];
    loop {
        // Outgoing commands.
        while let Ok(cmd) = cmd_rx.try_recv() {
            match cmd {
                ShareCmd::SetNowPlaying(name) => now_playing = name,
                ShareCmd::SetAllow(v) => allow = v,
                ShareCmd::SetName(n) => display_name = n,
                ShareCmd::Request { addr, sound } => {
                    let listener = TcpListener::bind(("0.0.0.0", 0)).ok();
                    if let Some(listener) = listener {
                        let port = listener.local_addr().map(|a| a.port()).unwrap_or(0);
                        let pkt = Packet {
                            t: "req".to_string(),
                            id: id.clone(),
                            name: display_name.clone(),
                            playing: None,
                            sound: Some(sound.clone()),
                            port: Some(port),
                        };
                        if let Ok(bytes) = serde_json::to_vec(&pkt) {
                            let _ = socket.send_to(&bytes, addr);
                        }
                        pending = Some((addr, sound, listener, std::time::Instant::now()));
                    }
                }
                ShareCmd::Respond { addr, file } => {
                    if let Some(path) = file {
                        if let Ok(data) = std::fs::read(&path) {
                            if let Ok(mut stream) = TcpStream::connect(addr) {
                                let name = path
                                    .file_name()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("sound.wav")
                                    .to_string();
                                let header = serde_json::json!({"name": name, "size": data.len()});
                                if let Ok(header_bytes) = serde_json::to_vec(&header) {
                                    let _ = stream.write_all(&(header_bytes.len() as u16).to_le_bytes());
                                    let _ = stream.write_all(&header_bytes);
                                    let _ = stream.write_all(&data);
                                }
                            }
                        }
                    } else {
                        let pkt = Packet {
                            t: "deny".to_string(),
                            id: id.clone(),
                            name: display_name.clone(),
                            playing: None,
                            sound: None,
                            port: None,
                        };
                        if let Ok(bytes) = serde_json::to_vec(&pkt) {
                            let _ = socket.send_to(&bytes, addr);
                        }
                    }
                }
            }
        }
        // Pending borrow receiver (non-blocking accept).
        if let Some((peer_addr, sound_name, listener, deadline)) = &pending {
            if deadline.elapsed() > Duration::from_secs(15) {
                let _ = msg_tx.send(ShareMsg::Note("Sound transfer timed out".to_string()));
                pending = None;
            } else {
                listener.set_nonblocking(true).ok();
                if let Ok((mut stream, _)) = listener.accept() {
                    let dir = sounds_dir.clone();
                    let msg_tx = msg_tx.clone();
                    std::thread::spawn(move || {
                        let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
                        match recv_file(&mut stream, &dir) {
                            Ok(path) => {
                                let name = path
                                    .file_stem()
                                    .and_then(|s| s.to_str())
                                    .unwrap_or("borrowed")
                                    .to_string();
                                let _ = msg_tx.send(ShareMsg::ReceivedFile { name, path });
                            }
                            _ => {
                                let _ = msg_tx.send(ShareMsg::Note("Sound transfer failed".to_string()));
                            }
                        }
                    });
                    let _ = (peer_addr, sound_name);
                    pending = None;
                }
            }
        }
        // Discovery heartbeat.
        if last_hello.elapsed() >= Duration::from_secs(HELLO_SECS) {
            last_hello = std::time::Instant::now();
            let pkt = Packet {
                t: "hello".to_string(),
                id: id.clone(),
                name: display_name.clone(),
                playing: now_playing.clone(),
                sound: None,
                port: None,
            };
            if let Ok(bytes) = serde_json::to_vec(&pkt) {
                let _ = socket.send_to(&bytes, broadcast);
            }
        }
        // Incoming datagrams.
        loop {
            match socket.recv_from(&mut buf) {
                Ok((len, addr)) => {
                    let Ok(pkt) = serde_json::from_slice::<Packet>(&buf[..len]) else {
                        continue;
                    };
                    if pkt.id == id {
                        continue; // ourselves
                    }
                    match pkt.t.as_str() {
                        "hello" => {
                            peers.insert(
                                pkt.id.clone(),
                                PeerInfo {
                                    id: pkt.id.clone(),
                                    name: pkt.name.clone(),
                                    now_playing: pkt.playing.clone(),
                                    addr,
                                    last_seen: unix_now(),
                                },
                            );
                        }
                        "req" => {
                            let sound = pkt.sound.clone().unwrap_or_default();
                            match pkt.port {
                                Some(port) if allow => {
                                    let _ = msg_tx.send(ShareMsg::IncomingRequest {
                                        addr: SocketAddr::new(addr.ip(), port),
                                        sound,
                                    });
                                }
                                _ => {
                                    let pkt = Packet {
                                        t: "deny".to_string(),
                                        id: id.clone(),
                                        name: display_name.clone(),
                                        playing: None,
                                        sound: None,
                                        port: None,
                                    };
                                    if let Ok(bytes) = serde_json::to_vec(&pkt) {
                                        let _ = socket.send_to(&bytes, addr);
                                    }
                                }
                            }
                        }
                        "deny" => {
                            let _ = msg_tx.send(ShareMsg::Note(format!("{} declined", pkt.name)));
                        }
                        _ => {}
                    }
                }
                Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(_) => break,
            }
        }
        // Prune stale peers and report changes.
        let now = unix_now();
        peers.retain(|_, p| now.saturating_sub(p.last_seen) <= PEER_TTL_SECS);
        let list: Vec<PeerInfo> = peers.values().cloned().collect();
        let _ = msg_tx.send(ShareMsg::Peers(list));
        std::thread::sleep(Duration::from_millis(120));
    }
}

fn udp_socket() -> std::io::Result<UdpSocket> {
    let socket = Socket::new(Domain::IPV4, Type::DGRAM, Some(Protocol::UDP))?;
    socket.set_reuse_address(true)?;
    socket.set_broadcast(true)?;
    socket.set_nonblocking(true)?;
    socket.bind(&SocketAddr::from(([0, 0, 0, 0], DISCOVERY_PORT)).into())?;
    Ok(UdpSocket::from(socket))
}

fn recv_file(stream: &mut TcpStream, dir: &std::path::Path) -> Result<PathBuf, String> {
    let _ = stream.set_read_timeout(Some(Duration::from_secs(30)));
    let mut header = [0u8; 2];
    stream.read_exact(&mut header).map_err(|e| e.to_string())?;
    let header_len = u16::from_le_bytes(header) as usize;
    let mut raw = vec![0u8; header_len];
    stream.read_exact(&mut raw).map_err(|e| e.to_string())?;
    let meta: serde_json::Value = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    let file_name = meta
        .get("name")
        .and_then(|v| v.as_str())
        .unwrap_or("borrowed.wav")
        .trim()
        .to_string();
    let size = meta.get("size").and_then(|v| v.as_u64()).unwrap_or(0);
    if size == 0 || size > FILE_CAP {
        return Err("bad file size".to_string());
    }
    let safe_name = file_name.replace(['/', '\\'], "_");
    let dest = dir.join(&safe_name);
    let mut file = std::fs::File::create(&dest).map_err(|e| e.to_string())?;
    let mut remaining = size;
    let mut buf = vec![0u8; 65536];
    while remaining > 0 {
        let to_read = remaining.min(buf.len() as u64) as usize;
        let n = stream.read(&mut buf[..to_read]).map_err(|e| e.to_string())?;
        if n == 0 {
            return Err("connection closed".to_string());
        }
        file.write_all(&buf[..n]).map_err(|e| e.to_string())?;
        remaining -= n as u64;
    }
    Ok(dest)
}
