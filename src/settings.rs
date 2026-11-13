//! User settings, persisted as JSON. Every field has a default so older or partial files still load.

use crate::errors::StorageError;
use crate::fsutil::{backup_corrupt, load_json, save_json, LoadStatus};
use crate::library::normalize_hotkey;
use serde::{Deserialize, Serialize};
use std::path::Path;

/// What happens when a sound that is already playing is triggered again.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RetriggerPolicy {
    /// Stop the running instance and start over.
    #[default]
    Restart,
    /// Ignore the new trigger while the sound is still playing.
    Ignore,
    /// Start an additional overlapping instance.
    Overlap,
}

#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum CloseBehavior {
    /// Closing the window quits VibeDrop (and releases all hotkeys).
    #[default]
    Exit,
    /// Closing the window minimizes it so hotkeys keep working.
    Minimize,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// Headphones / speakers. Device names as reported by the audio host; `None` means the system default.
    pub main_output: Option<String>,
    /// The microphone route: the output device (normally a virtual audio cable) whose recording
    /// side is used as the microphone in Discord, games, etc. `None` = nothing selected.
    /// (The field names are kept from older versions so existing settings files still load.)
    pub secondary_output: Option<String>,
    /// Whether sounds are also sent to the microphone route.
    pub secondary_enabled: bool,
    pub master_volume: f32,
    /// Volume of the headphones route (0..=1), on top of the master volume.
    pub main_volume: f32,
    /// Volume of the microphone route (0..=1), on top of the master volume.
    pub mic_volume: f32,
    /// Quick mute of the microphone route; normal playback continues.
    pub mic_muted: bool,
    /// Whether the user's real microphone is mixed into the microphone route, so that a voice chat
    /// hears voice and sounds through one device. Off by default: VibeDrop does not open a
    /// microphone unless asked to.
    pub voice_enabled: bool,
    /// The real microphone to mix in (a recording device, never an output). `None` = the system
    /// default microphone.
    pub voice_input: Option<String>,
    /// Volume of the user's voice in the microphone route (0..=1). Independent of the sound volumes.
    pub voice_volume: f32,
    /// Global hotkey that toggles `mic_muted`, in canonical form (e.g. `Ctrl+Alt+M`).
    pub mic_mute_hotkey: Option<String>,
    /// Gain applied to every newly started sound.
    pub default_sound_volume: f32,
    /// If false, starting a sound stops all others first.
    pub allow_overlap: bool,
    pub retrigger: RetriggerPolicy,
    /// UI language code. Only "en" ships strings in Part 1.
    pub language: String,
    pub close_behavior: CloseBehavior,
    /// Copy imported files into the VibeDrop sounds folder.
    pub copy_on_import: bool,
    /// When removing a library entry, also delete the VibeDrop-managed copy (never the user's original).
    pub delete_managed_copy_on_remove: bool,
    /// Appearance: colour theme, accent colour and UI density.
    pub theme: ThemeSettings,
    /// Behaviour of the sound table and the player.
    pub playback: PlaybackSettings,
    /// Music section: external tool paths, optional Spotify Web API credentials and volume.
    pub music: MusicSettings,
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct ThemeSettings {
    /// `dark` (classic soundboard grey) or `midnight` (deep blue-black).
    pub variant: String,
    /// Accent colour as `#RRGGBB`; empty = the built-in accent.
    pub accent: String,
    /// Corner rounding of cards and panels, in pixels (0..=24).
    pub corner_radius: f32,
    /// Base font size in points (12..=20).
    pub font_scale: f32,
    /// Show the icon labels in the sidebar.
    pub show_sidebar_labels: bool,
    /// Show the transport strip under the menu bar.
    pub show_transport: bool,
    /// Show the status bar at the bottom.
    pub show_status_bar: bool,
    /// Dim the window while a sound plays (visual feedback).
    pub flash_on_play: bool,
    /// Show a waveform/level meter in the transport strip.
    pub show_level_meter: bool,
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            variant: "dark".to_string(),
            accent: String::new(),
            corner_radius: 8.0,
            font_scale: 15.0,
            show_sidebar_labels: true,
            show_transport: true,
            show_status_bar: true,
            flash_on_play: false,
            show_level_meter: false,
        }
    }
}

#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct PlaybackSettings {
    /// Play the sound that was played last when the app starts.
    pub resume_last_sound: bool,
    /// Seconds of the sound to skip when it starts (useful for long intros).
    pub start_offset_secs: f64,
    /// Fade in/out time in milliseconds.
    pub fade_ms: u32,
    /// Confirm before removing a sound from the library.
    pub confirm_remove: bool,
    /// Show the sound duration in the table.
    pub show_durations: bool,
    /// Stop all sounds when the window loses focus.
    pub stop_on_focus_loss: bool,
    /// Maximum number of sounds that may play at the same time (1..=32).
    pub max_voices: u32,
}

impl Default for PlaybackSettings {
    fn default() -> Self {
        Self {
            resume_last_sound: false,
            start_offset_secs: 0.0,
            fade_ms: 0,
            confirm_remove: false,
            show_durations: true,
            stop_on_focus_loss: false,
            max_voices: 8,
        }
    }
}

/// Settings of the Music section. All paths are optional: an empty string means "find the
/// program on PATH" and the two Spotify fields are empty when the public page is used.
#[derive(Serialize, Deserialize, Clone, Debug, PartialEq)]
#[serde(default)]
pub struct MusicSettings {
    /// Explicit path to `yt-dlp` (a file or the folder that contains it).
    pub yt_dlp_path: String,
    /// Explicit path to `ffmpeg` (a file or the folder that contains it).
    pub ffmpeg_path: String,
    /// Spotify Web API Client ID. Empty = read the public album page instead.
    pub spotify_client_id: String,
    /// Spotify Web API Client Secret. Stored locally in plain text; a Client Credentials secret
    /// only grants read access to the public catalogue.
    pub spotify_client_secret: String,
    /// Volume of the Music section, on top of the master and route volumes (0..=1).
    pub volume: f32,
}

impl Default for MusicSettings {
    fn default() -> Self {
        Self {
            yt_dlp_path: String::new(),
            ffmpeg_path: String::new(),
            spotify_client_id: String::new(),
            spotify_client_secret: String::new(),
            volume: 0.8,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            main_output: None,
            secondary_output: None,
            secondary_enabled: false,
            master_volume: 1.0,
            main_volume: 1.0,
            mic_volume: 1.0,
            mic_muted: false,
            voice_enabled: false,
            voice_input: None,
            voice_volume: 1.0,
            mic_mute_hotkey: None,
            default_sound_volume: 1.0,
            allow_overlap: true,
            retrigger: RetriggerPolicy::Restart,
            language: "en".to_string(),
            close_behavior: CloseBehavior::Exit,
            copy_on_import: true,
            delete_managed_copy_on_remove: false,
            theme: ThemeSettings::default(),
            playback: PlaybackSettings::default(),
            music: MusicSettings::default(),
        }
    }
}

fn clamp_unit(v: f32, fallback: f32) -> f32 {
    if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        fallback
    }
}

fn clean_device(name: Option<String>) -> Option<String> {
    name.filter(|n| !n.trim().is_empty())
}

/// True for a `#RRGGBB` / `#RGB` colour string.
fn is_hex_color(text: &str) -> bool {
    let t = text.trim();
    let hex = t.strip_prefix('#').unwrap_or(t);
    matches!(hex.len(), 3 | 6) && hex.chars().all(|c| c.is_ascii_hexdigit())
}

impl Settings {
    /// Build settings from a parsed JSON object, field by field: a field with a wrong type or an
    /// unknown name is ignored (defaults are kept) instead of resetting the whole file.
    fn from_object(user: &serde_json::Map<String, serde_json::Value>) -> Settings {
        use serde_json::Value;
        let defaults = match serde_json::to_value(Settings::default()) {
            Ok(Value::Object(m)) => m,
            _ => return Settings::default(),
        };
        let mut merged = defaults.clone();
        for (key, value) in user {
            if !defaults.contains_key(key) {
                continue;
            }
            let mut trial = merged.clone();
            trial.insert(key.clone(), value.clone());
            if serde_json::from_value::<Settings>(Value::Object(trial.clone())).is_ok() {
                merged = trial;
            } else {
                tracing::warn!("settings: ignoring invalid value for \"{key}\"");
            }
        }
        serde_json::from_value(Value::Object(merged)).unwrap_or_default()
    }

    /// Bring hand-edited or corrupt values back into valid ranges.
    pub fn sanitize(&mut self) {
        self.master_volume = clamp_unit(self.master_volume, 1.0);
        self.main_volume = clamp_unit(self.main_volume, 1.0);
        self.mic_volume = clamp_unit(self.mic_volume, 1.0);
        self.voice_volume = clamp_unit(self.voice_volume, 1.0);
        self.voice_input = clean_device(self.voice_input.take());
        self.main_output = clean_device(self.main_output.take());
        self.secondary_output = clean_device(self.secondary_output.take());
        self.mic_mute_hotkey = self.mic_mute_hotkey.take().and_then(|h| normalize_hotkey(&h).ok());
        self.default_sound_volume = clamp_unit(self.default_sound_volume, 1.0);
        if self.language.trim().is_empty() {
            self.language = "en".to_string();
        }
        // Appearance.
        if !matches!(self.theme.variant.as_str(), "dark" | "midnight" | "light") {
            self.theme.variant = "dark".to_string();
        }
        if !self.theme.accent.is_empty() && !is_hex_color(&self.theme.accent) {
            self.theme.accent.clear();
        }
        self.theme.corner_radius = self.theme.corner_radius.clamp(0.0, 24.0);
        self.theme.font_scale = if self.theme.font_scale.is_finite() {
            self.theme.font_scale.clamp(12.0, 20.0)
        } else {
            15.0
        };
        // Playback.
        self.playback.start_offset_secs = if self.playback.start_offset_secs.is_finite() {
            self.playback.start_offset_secs.clamp(0.0, 600.0)
        } else {
            0.0
        };
        self.playback.fade_ms = self.playback.fade_ms.min(2000);
        self.playback.max_voices = self.playback.max_voices.clamp(1, 32);
        // Music: trim paths/keys and keep the volume in range.
        let trim = |s: &mut String| {
            let t = s.trim();
            if t.len() > 2048 {
                *s = t.chars().take(2048).collect();
            } else if t.len() != s.len() {
                *s = t.to_string();
            }
        };
        trim(&mut self.music.yt_dlp_path);
        trim(&mut self.music.ffmpeg_path);
        trim(&mut self.music.spotify_client_id);
        trim(&mut self.music.spotify_client_secret);
        self.music.volume = clamp_unit(self.music.volume, 0.8);
    }

    pub fn load(path: &Path) -> Result<(Settings, LoadStatus), StorageError> {
        let (value, status): (serde_json::Value, LoadStatus) = load_json(path)?;
        let (mut s, status) = match (value, status) {
            (serde_json::Value::Object(map), status) => (Settings::from_object(&map), status),
            (serde_json::Value::Null, status) => (Settings::default(), status),
            (_, _) => {
                // Valid JSON, but not an object: treat the file as damaged.
                let backup = backup_corrupt(path);
                (
                    Settings::default(),
                    LoadStatus::Recovered {
                        backup,
                        reason: "the file does not contain a settings object".to_string(),
                    },
                )
            }
        };
        s.sanitize();
        Ok((s, status))
    }

    pub fn save(&self, path: &Path) -> Result<(), StorageError> {
        save_json(path, self)
    }
}
