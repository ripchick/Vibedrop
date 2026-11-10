//! Download helper for the free VB-CABLE virtual audio cable.
//!
//! VB-CABLE is freeware by VB-Audio. The package is fetched from the official
//! server at the user's explicit request; it is never bundled with VibeDrop
//! and is not redistributed by this project.

use std::path::Path;

/// Official download location of the VB-CABLE driver pack (VB-Audio).
pub const VB_CABLE_URL: &str = "https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip";
/// Where the user can read about the product and its license.
pub const VB_CABLE_PAGE: &str = "https://vb-audio.com/Cable/";

/// Download the VB-CABLE package to `dest`, reporting `(received, total)` bytes.
/// The file is written as it arrives, so an interrupted download leaves a partial
/// file that the caller can delete.
pub fn download(dest: &Path, mut progress: impl FnMut(u64, u64)) -> Result<(), String> {
    use std::io::{Read, Write};
    let client = reqwest::blocking::Client::builder()
        .user_agent("VibeDrop/0.11 (desktop soundboard; free cable helper)")
        .timeout(std::time::Duration::from_secs(300))
        .build()
        .map_err(|e| e.to_string())?;
    let mut resp = client.get(VB_CABLE_URL).send().map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let total = resp.content_length().unwrap_or(0);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let mut file = std::fs::File::create(dest).map_err(|e| e.to_string())?;
    let mut buf = vec![0u8; 64 * 1024];
    let mut received = 0u64;
    loop {
        let n = resp.read(&mut buf).map_err(|e| e.to_string())?;
        if n == 0 {
            break;
        }
        std::io::Write::write_all(&mut file, &buf[..n]).map_err(|e| e.to_string())?;
        received += n as u64;
        progress(received, total);
    }
    file.flush().map_err(|e| e.to_string())?;
    Ok(())
}
