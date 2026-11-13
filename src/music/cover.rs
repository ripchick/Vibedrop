//! Album covers for the Music section.
//!
//! The cover of an album is fetched once from the address that came with the metadata and is
//! kept in the data folder, so the next start needs no network. Downloading and decoding happen
//! in a background thread; the UI only ever receives finished pixels and turns them into a
//! texture.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Receiver, Sender};

use eframe::egui::{ColorImage, Context, TextureHandle, TextureOptions};

/// The edge length the covers are scaled to. 256 px is plenty for a 50 px tile and keeps the
/// texture small (256 Г— 256 Г— 4 bytes).
const COVER_SIZE: u32 = 256;

/// A cover that is ready to be shown.
struct Decoded {
    url: String,
    width: usize,
    height: usize,
    pixels: Vec<u8>,
}

/// Downloaded and decoded album covers.
pub struct CoverCache {
    dir: PathBuf,
    ctx: Context,
    textures: HashMap<String, TextureHandle>,
    pending: HashSet<String>,
    rx: Receiver<Decoded>,
    tx: Sender<Decoded>,
}

impl CoverCache {
    pub fn new(dir: PathBuf, ctx: Context) -> Self {
        let (tx, rx) = channel();
        Self {
            dir,
            ctx,
            textures: HashMap::new(),
            pending: HashSet::new(),
            rx,
            tx,
        }
    }

    /// The texture of a cover, when it is already loaded.
    ///
    /// The first call starts the download in the background; the picture appears as soon as it
    /// is there.
    pub fn get(&mut self, url: &str) -> Option<TextureHandle> {
        let url = url.trim();
        if url.is_empty() {
            return None;
        }
        if let Some(texture) = self.textures.get(url) {
            return Some(texture.clone());
        }
        self.request(url);
        None
    }

    /// Whether a cover is on its way.
    pub fn is_loading(&self, url: &str) -> bool {
        self.pending.contains(url.trim())
    }

    fn request(&mut self, url: &str) {
        if !self.pending.insert(url.to_string()) {
            return;
        }
        let dir = self.dir.clone();
        let tx = self.tx.clone();
        let url = url.to_string();
        std::thread::spawn(move || {
            if let Some(decoded) = load_or_download(&url, &dir) {
                let _ = tx.send(decoded);
            }
        });
    }

    /// Turn the covers that finished into textures. Call once per frame.
    pub fn poll(&mut self) {
        while let Ok(decoded) = self.rx.try_recv() {
            self.pending.remove(&decoded.url);
            if decoded.pixels.is_empty() {
                continue;
            }
            let size = [decoded.width, decoded.height];
            let image = ColorImage::from_rgba_unmultiplied(size, &decoded.pixels);
            let handle = self
                .ctx
                .load_texture(format!("cover:{}", decoded.url), image, TextureOptions::LINEAR);
            self.textures.insert(decoded.url, handle);
        }
    }
}

/// The file a cover is cached in. The name is derived from the address, so the same album always
/// ends up in the same file.
fn cache_file(dir: &Path, url: &str) -> PathBuf {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for byte in url.as_bytes() {
        hash ^= u64::from(*byte);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    dir.join(format!("{hash:016x}.img"))
}

/// Read the cover from the cache folder, or download it once and store it there.
fn load_or_download(url: &str, dir: &Path) -> Option<Decoded> {
    let _ = std::fs::create_dir_all(dir);
    let file = cache_file(dir, url);
    let bytes = match std::fs::read(&file) {
        Ok(bytes) => bytes,
        Err(_) => {
            let bytes = download(url)?;
            // A cover that cannot be cached is still worth showing.
            let _ = std::fs::write(&file, &bytes);
            bytes
        }
    };
    decode(url, &bytes)
}

fn download(url: &str) -> Option<Vec<u8>> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(20))
        .user_agent("VibeDrop/0.13 (album covers)")
        .build()
        .ok()?;
    let response = client.get(url).send().ok()?;
    if !response.status().is_success() {
        return None;
    }
    let bytes = response.bytes().ok()?;
    // A cover is a small image; anything bigger is not one.
    if bytes.len() > 8 * 1024 * 1024 {
        return None;
    }
    Some(bytes.to_vec())
}

/// Decode an image and scale it down to a size that is enough for the UI.
fn decode(url: &str, bytes: &[u8]) -> Option<Decoded> {
    let image = image::load_from_memory(bytes).ok()?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    let scaled = if width > COVER_SIZE || height > COVER_SIZE {
        image::imageops::resize(&rgba, COVER_SIZE, COVER_SIZE, image::imageops::FilterType::Triangle)
    } else {
        rgba
    };
    let (width, height) = scaled.dimensions();
    Some(Decoded {
        url: url.to_string(),
        width: width as usize,
        height: height as usize,
        pixels: scaled.into_raw(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_cache_file_depends_on_the_address() {
        let dir = PathBuf::from("cache");
        let a = cache_file(&dir, "https://example.com/a.jpg");
        let b = cache_file(&dir, "https://example.com/b.jpg");
        assert_ne!(a, b);
        assert_eq!(a, cache_file(&dir, "https://example.com/a.jpg"));
        assert_eq!(a.extension().unwrap(), "img");
    }

    #[test]
    fn a_broken_image_is_skipped() {
        assert!(decode("x", b"not an image").is_none());
    }
}
