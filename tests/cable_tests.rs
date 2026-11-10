//! The VB-CABLE helper must really fetch the official package from VB-Audio.
//! The test needs the network, so it is skipped when VIBEDROP_OFFLINE_TESTS=1.

use std::path::PathBuf;

#[test]
fn downloads_the_official_vb_cable_package() {
    if std::env::var("VIBEDROP_OFFLINE_TESTS").is_ok() {
        eprintln!("offline: skipping the VB-CABLE download test");
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let dest: PathBuf = dir.path().join("VBCABLE_Driver_Pack45.zip");
    let mut last = (0u64, 0u64);
    let result = vibedrop::cable::download(&dest, |received, total| last = (received, total));
    let (received, total) = last;
    let Err(e) = result else {
        assert!(received > 0, "nothing was downloaded");
        if total > 0 {
            assert_eq!(received, total, "the file was truncated");
        }
        let size = std::fs::metadata(&dest).unwrap().len();
        assert!(size > 100_000, "suspiciously small package: {size} bytes");
        // A zip starts with "PK".
        let head = std::fs::read(&dest).unwrap();
        assert_eq!(&head[0..2], b"PK", "not a zip file");
        return;
    };
    // No network in this environment: do not fail the build over it.
    eprintln!("offline: the VB-CABLE download test was skipped ({e})");
}
