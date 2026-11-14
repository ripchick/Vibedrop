//! Checks whether a newer VibeDrop is available.
//!
//! The check runs in the background and only compares the version numbers of the latest
//! release on GitHub with the version of this build. Nothing is downloaded or changed.

use std::time::Duration;

/// Where the newest version is published.
pub const RELEASES_PAGE: &str = "https://github.com/ripchick/Vibedrop/releases/latest";
/// The API that reports the newest release.
const LATEST_RELEASE_API: &str = "https://api.github.com/repos/ripchick/Vibedrop/releases/latest";

/// The version of this build.
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// True when `latest` is newer than `current`.
///
/// Versions are compared part by part ("0.14.2" > "0.14.1" > "0.13.9"); a version that cannot
/// be read is ignored, so a strange tag never suggests an update.
pub fn is_newer(latest: &str, current: &str) -> bool {
    let parse = |v: &str| -> Vec<u64> {
        v.trim()
            .trim_start_matches('v')
            .split(['.', '-'])
            .map(|part| part.parse::<u64>().unwrap_or(0))
            .collect()
    };
    let mut latest = parse(latest);
    let mut current = parse(current);
    let len = latest.len().max(current.len());
    latest.resize(len, 0);
    current.resize(len, 0);
    latest > current
}

/// Ask GitHub for the newest release. Returns its version.
pub fn latest_version() -> Result<String, String> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent(concat!("VibeDrop/", env!("CARGO_PKG_VERSION")))
        .build()
        .map_err(|e| e.to_string())?;
    let response = client.get(LATEST_RELEASE_API).send().map_err(|e| e.to_string())?;
    if !response.status().is_success() {
        return Err(format!("GitHub answered with {}", response.status()));
    }
    let value: serde_json::Value = response.json().map_err(|e| e.to_string())?;
    let tag = value
        .get("tag_name")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "the release has no version".to_string())?;
    Ok(tag.trim_start_matches('v').to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn newer_versions_are_recognised() {
        assert!(is_newer("0.14.2", "0.14.1"));
        assert!(is_newer("0.15.0", "0.14.9"));
        assert!(is_newer("1.0.0", "0.14.1"));
        assert!(is_newer("v0.14.2", "0.14.1"));
    }

    #[test]
    fn the_same_or_an_older_version_is_not_an_update() {
        assert!(!is_newer("0.14.1", "0.14.1"));
        assert!(!is_newer("0.14.0", "0.14.1"));
        assert!(!is_newer("0.9.9", "0.10.0"));
    }

    #[test]
    fn a_broken_version_is_ignored() {
        assert!(!is_newer("nightly", "0.14.1"));
        assert!(!is_newer("", "0.14.1"));
    }
}
