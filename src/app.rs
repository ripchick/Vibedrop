//! The VibeDrop window (egui / eframe).
//!
//! Layout: a sidebar (Library, Favorites, History, Settings) on the left, the current
//! page in the center and a player bar pinned to the bottom. All state changes go through
//! `VibeDropApp` methods; the drawing code only collects "actions" and the update loop
//! applies them afterwards.

// `ComboBox::from_id_source` was renamed in newer egui releases; the old name works in all of them.
#![allow(deprecated)]

use crate::audio::engine::{
    DeviceConfig, Engine, EngineEvent, Levels, OutputRole, ReportLevel, RouteState, RouteTestReport,
};
use crate::audio::routing::{
    explain_device_list, filter_route_output_names, find_paired_input, is_virtual_device, sort_for_mic_route,
    sort_for_voice_input, DeviceKind,
};
use crate::downloader::{self, DownloadEvent, DownloadHandle};
use crate::errors::DownloadError;
use crate::fsutil::unix_now;
use crate::hotkeys::Hotkeys;
use crate::lang;
use crate::library::{
    check_available, file_fingerprint, import_file, normalize_hotkey, remove_managed_file, validate_audio_file,
    ImportSource, Library, SoundEntry, MAX_ENTRY_VOLUME,
};
use crate::paths::AppPaths;
use crate::protocol::{parse_uri, parse_url_text, ProtocolCommand};
use crate::settings::{CloseBehavior, RetriggerPolicy, Settings};
use crate::system::{self, ProtocolState, WakeHook};
use crate::theme::{self, Icon};
use crossbeam_channel::{unbounded, Receiver, Sender};
use eframe::egui::{self, vec2, Align, Align2, Color32, Key, Layout, RichText, Rounding, Sense, Stroke, Ui};
use std::collections::{HashMap, HashSet, VecDeque};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use url::Url;

const HISTORY_MAX: usize = 100;
const TOAST_LIFETIME: Duration = Duration::from_secs(5);
const MAX_TOASTS: usize = 5;
const MAX_FAILED_DOWNLOADS: usize = 5;
/// Pseudo sound id under which the "mute microphone route" hotkey is registered.
const MIC_TOGGLE_ID: &str = "__action:mic_mute_toggle";

fn lock<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Everything `main` prepares before the window opens.
pub struct Startup {
    pub paths: AppPaths,
    pub settings: Settings,
    pub library: Library,
    /// Problems found while loading (corrupt files etc.), shown as notifications.
    pub notices: Vec<String>,
    /// A `soundpad://` link given on the command line.
    pub initial_uri: Option<String>,
    /// Links forwarded by later launches of VibeDrop.
    pub ipc_rx: Receiver<String>,
    pub wake: WakeHook,
}

struct PlayInfo {
    path: PathBuf,
    volume: f32,
}

/// Data the hotkey thread needs to start a sound without touching the GUI.
#[derive(Default)]
struct PlayIndex {
    sounds: HashMap<String, PlayInfo>,
    gain: f32,
    allow_overlap: bool,
    retrigger: RetriggerPolicy,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Library,
    Favorites,
    History,
    Settings,
}

impl Page {
    const ALL: [Page; 4] = [Page::Library, Page::Favorites, Page::History, Page::Settings];

    fn title(self) -> &'static str {
        match self {
            Page::Library => "Library",
            Page::Favorites => "Favorites",
            Page::History => "History",
            Page::Settings => "Settings",
        }
    }

    fn icon(self) -> Icon {
        match self {
            Page::Library => Icon::Library,
            Page::Favorites => Icon::Star,
            Page::History => Icon::History,
            Page::Settings => Icon::Settings,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum ToastKind {
    Info,
    Success,
    Error,
}

struct Toast {
    kind: ToastKind,
    text: String,
    until: Instant,
}

struct HistoryItem {
    sound_id: String,
    name: String,
    at: u64,
}

/// Messages from background helper threads.
enum HelperMsg {
    Cable(Result<(), String>),
    /// Progress of the VB-CABLE download: (received, total).
    CableProgress(u64, u64),
    /// The VB-CABLE package was downloaded to this path.
    CableReady(String),
}

/// State of the first-run setup wizard.
#[derive(Default, Clone)]
struct SetupState {
    download_cable: bool,
    cable_progress: f32,
    cable_status: String,
    cable_done: bool,
    desktop_shortcut: bool,
    autostart: bool,
    busy: bool,
}

struct ActiveDownload {
    handle: DownloadHandle,
    received: u64,
    total: Option<u64>,
    label: String,
    play_after: bool,
    url: Url,
}

/// A download that ended with an error; kept on the Library page until dismissed.
struct FailedDownload {
    label: String,
    url: Url,
    error: String,
    play_after: bool,
}

struct RenameDialog {
    id: String,
    text: String,
    focus: bool,
}

struct HotkeyDialog {
    id: String,
    name: String,
    text: String,
    recording: bool,
    error: Option<String>,
}

struct UrlDialog {
    text: String,
    error: Option<String>,
    focus: bool,
}

enum Action {
    Play(String),
    Stop(String),
    SetVolume(String, f32),
    Relink(String),
    ToggleFavorite(String),
    Rename(String),
    EditHotkey(String),
    ClearHotkey(String),
    Remove(String),
    ExtractAudio(String),
}

/// Owned snapshot of one library entry for drawing.
struct Row {
    id: String,
    name: String,
    meta: String,
    hotkey: Option<String>,
    hotkey_inactive: bool,
    favorite: bool,
    playing: bool,
    missing: bool,
    volume: f32,
    /// True when the source file is a video (MP4/MOV) with an audio track.
    video: bool,
}

pub struct VibeDropApp {
    paths: AppPaths,
    settings: Settings,
    library: Library,
    engine: Engine,
    hotkeys: Option<Hotkeys>,
    hotkey_error: Option<String>,
    /// Sounds whose hotkey could not be registered (taken by another program).
    inactive_hotkeys: HashSet<String>,
    mic_hotkey_inactive: bool,
    play_index: Arc<Mutex<PlayIndex>>,
    hk_rx: Receiver<String>,
    ipc_rx: Receiver<String>,
    wake: WakeHook,

    page: Page,
    search: String,
    history: VecDeque<HistoryItem>,
    now_playing: Option<String>,
    missing: HashSet<String>,
    toasts: Vec<Toast>,

    failed_downloads: Vec<FailedDownload>,
    route_test: Option<RouteTestReport>,
    route_test_running: bool,

    downloads: HashMap<u64, ActiveDownload>,
    next_download: u64,
    dl_tx: Sender<DownloadEvent>,
    dl_rx: Receiver<DownloadEvent>,

    helper_rx: Receiver<HelperMsg>,
    helper_tx: Sender<HelperMsg>,

    discord: crate::discord::DiscordPresence,
    applied_theme: crate::settings::ThemeSettings,
    setup: Option<SetupState>,

    rename: Option<RenameDialog>,
    hotkey_dialog: Option<HotkeyDialog>,
    url_dialog: Option<UrlDialog>,

    protocol_state: ProtocolState,

    settings_dirty: bool,
    library_dirty: bool,
    quitting: bool,
    about: bool,
    shut_down: bool,
}

impl VibeDropApp {
    pub fn new(cc: &eframe::CreationContext<'_>, startup: Startup) -> Self {
        theme::apply_settings(&cc.egui_ctx, &startup.settings.theme);
        if let Ok(mut guard) = startup.wake.lock() {
            *guard = Some(cc.egui_ctx.clone());
        }
        let Startup {
            paths,
            settings,
            library,
            notices,
            initial_uri,
            ipc_rx,
            wake,
        } = startup;

        let engine = Engine::start(
            DeviceConfig {
                main: settings.main_output.clone(),
                secondary: settings.secondary_output.clone(),
                secondary_enabled: settings.secondary_enabled,
                voice: settings.voice_input.clone(),
                voice_enabled: settings.voice_enabled,
            },
            Levels {
                master: settings.master_volume,
                main: settings.main_volume,
                mic: settings.mic_volume,
                mic_muted: settings.mic_muted,
                voice: settings.voice_volume,
            },
        );
        let (hk_tx, hk_rx) = unbounded::<String>();
        let (dl_tx, dl_rx) = unbounded::<DownloadEvent>();
        let applied_theme = settings.theme.clone();
        let setup_done = settings.setup_done;
        let (helper_tx, helper_rx) = unbounded::<HelperMsg>();
        let discord = crate::discord::DiscordPresence::start();
        let missing: HashSet<String> = library
            .entries
            .iter()
            .filter(|e| check_available(e).is_err())
            .map(|e| e.id.clone())
            .collect();

        let mut app = Self {
            paths,
            settings,
            library,
            engine,
            hotkeys: None,
            hotkey_error: None,
            inactive_hotkeys: HashSet::new(),
            mic_hotkey_inactive: false,
            play_index: Arc::new(Mutex::new(PlayIndex::default())),
            hk_rx,
            ipc_rx,
            wake,
            page: Page::Library,
            search: String::new(),
            history: VecDeque::new(),
            now_playing: None,
            missing,
            toasts: Vec::new(),
            failed_downloads: Vec::new(),
            route_test: None,
            route_test_running: false,
            downloads: HashMap::new(),
            next_download: 1,
            dl_tx,
            dl_rx,
            helper_rx,
            helper_tx,
            discord,
            applied_theme,
            setup: if setup_done { None } else { Some(SetupState::default()) },
            rename: None,
            hotkey_dialog: None,
            url_dialog: None,
            protocol_state: system::protocol_state(),
            settings_dirty: false,
            library_dirty: false,
            quitting: false,
            about: false,
            shut_down: false,
        };
        app.sync_index();
        app.load_history();
        #[cfg(not(target_os = "android"))]
        app.init_hotkeys(hk_tx);
        #[cfg(target_os = "android")]
        drop(hk_tx);
        for notice in notices {
            app.toast(ToastKind::Error, lang::tx(&app.settings.language, &notice));
        }
        if let Some(uri) = initial_uri {
            app.handle_link(&uri);
        }
        app
    }

    // ---- Setup and persistence ---------------------------------------------------------------

    fn init_hotkeys(&mut self, hk_tx: Sender<String>) {
        match Hotkeys::new() {
            Ok(mut hk) => {
                let engine = self.engine.clone();
                let index = Arc::clone(&self.play_index);
                let wake = Arc::clone(&self.wake);
                hk.start_listener(move |sound_id| {
                    if sound_id == MIC_TOGGLE_ID {
                        engine.toggle_mic_muted();
                        let _ = hk_tx.send(sound_id.to_string());
                        system::wake(&wake);
                        return;
                    }
                    let (path, gain, allow_overlap, policy) = {
                        let g = lock(&index);
                        match g.sounds.get(sound_id) {
                            Some(info) => (info.path.clone(), g.gain * info.volume, g.allow_overlap, g.retrigger),
                            None => return,
                        }
                    };
                    engine.play(sound_id, &path, gain, policy, allow_overlap);
                    let _ = hk_tx.send(sound_id.to_string());
                    system::wake(&wake);
                });
                let mut wanted: Vec<(String, String)> = self
                    .library
                    .entries
                    .iter()
                    .filter_map(|e| e.hotkey.clone().map(|h| (e.id.clone(), h)))
                    .collect();
                if let Some(h) = self.settings.mic_mute_hotkey.clone() {
                    wanted.push((MIC_TOGGLE_ID.to_string(), h));
                }
                let lang = self.settings.language.clone();
                for (id, reason) in hk.sync(&wanted) {
                    if id == MIC_TOGGLE_ID {
                        self.mic_hotkey_inactive = true;
                        let text = if lang::ru(&lang) {
                            format!(
                                "Горячая клавиша отключения микрофона не работает: {}",
                                lang::tx(&lang, &reason)
                            )
                        } else {
                            format!("The microphone mute hotkey is inactive: {reason}")
                        };
                        self.toast(ToastKind::Error, text);
                        continue;
                    }
                    let name = self.library.get(&id).map(|e| e.name.clone()).unwrap_or_default();
                    self.inactive_hotkeys.insert(id);
                    let text = if lang::ru(&lang) {
                        format!("Горячая клавиша для «{name}» не работает: {}", lang::tx(&lang, &reason))
                    } else {
                        format!("Hotkey of \"{name}\" is inactive: {reason}")
                    };
                    self.toast(ToastKind::Error, text);
                }
                self.hotkeys = Some(hk);
            }
            Err(e) => {
                tracing::error!("{e}");
                self.hotkey_error = Some(e);
            }
        }
    }

    fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        let text = text.into();
        if kind == ToastKind::Error {
            tracing::warn!("{text}");
        }
        self.toasts.push(Toast {
            kind,
            text,
            until: Instant::now() + TOAST_LIFETIME,
        });
        if self.toasts.len() > MAX_TOASTS {
            self.toasts.remove(0);
        }
    }

    fn save_library(&mut self) {
        self.library_dirty = false;
        if let Err(e) = self.library.save(&self.paths.library_file) {
            let text = if lang::ru(&self.settings.language) {
                format!(
                    "Не удалось сохранить библиотеку: {}",
                    lang::tx(&self.settings.language, &e.to_string())
                )
            } else {
                format!("Could not save the library: {e}")
            };
            self.toast(ToastKind::Error, text);
        }
    }

    fn save_settings(&mut self) {
        self.settings_dirty = false;
        if let Err(e) = self.settings.save(&self.paths.settings_file) {
            let text = if lang::ru(&self.settings.language) {
                format!(
                    "Не удалось сохранить настройки: {}",
                    lang::tx(&self.settings.language, &e.to_string())
                )
            } else {
                format!("Could not save the settings: {e}")
            };
            self.toast(ToastKind::Error, text);
        }
    }

    /// Refresh the data used by the hotkey thread.
    fn sync_index(&self) {
        let mut g = lock(&self.play_index);
        g.sounds = self
            .library
            .entries
            .iter()
            .map(|e| {
                (
                    e.id.clone(),
                    PlayInfo {
                        path: e.path.clone(),
                        volume: e.volume,
                    },
                )
            })
            .collect();
        g.gain = self.settings.default_sound_volume;
        g.allow_overlap = self.settings.allow_overlap;
        g.retrigger = self.settings.retrigger;
    }

    fn device_config(&self) -> DeviceConfig {
        DeviceConfig {
            main: self.settings.main_output.clone(),
            secondary: self.settings.secondary_output.clone(),
            secondary_enabled: self.settings.secondary_enabled,
            voice: self.settings.voice_input.clone(),
            voice_enabled: self.settings.voice_enabled,
        }
    }

    fn levels(&self) -> Levels {
        Levels {
            master: self.settings.master_volume,
            main: self.settings.main_volume,
            mic: self.settings.mic_volume,
            mic_muted: self.settings.mic_muted,
            voice: self.settings.voice_volume,
        }
    }

    fn on_settings_changed(&mut self, before: &Settings) {
        self.settings.sanitize();
        self.settings_dirty = true;
        self.engine.set_levels(self.levels());
        if before.main_output != self.settings.main_output
            || before.secondary_output != self.settings.secondary_output
            || before.secondary_enabled != self.settings.secondary_enabled
            || before.voice_input != self.settings.voice_input
            || before.voice_enabled != self.settings.voice_enabled
        {
            self.route_test = None;
            self.engine.configure(self.device_config());
        }
        self.sync_index();
    }

    fn toggle_mic_mute(&mut self) {
        self.settings.mic_muted = !self.settings.mic_muted;
        self.engine.set_levels(self.levels());
        self.settings_dirty = true;
    }

    /// Idempotent clean-up: release hotkeys and audio devices, flush data to disk.
    fn shutdown(&mut self) {
        if self.shut_down {
            return;
        }
        self.shut_down = true;
        if let Some(mut hk) = self.hotkeys.take() {
            hk.unregister_all();
        }
        for dl in self.downloads.values() {
            dl.handle.cancel();
        }
        self.engine.stop_all();
        self.engine.shutdown();
        if self.library_dirty {
            self.save_library();
        }
        if self.settings_dirty {
            self.save_settings();
        }
    }

    // ---- Playback ----------------------------------------------------------------------------

    fn play_sound(&mut self, id: &str) {
        let Some(entry) = self.library.get(id) else { return };
        if let Err(e) = check_available(entry) {
            let name = entry.name.clone();
            self.missing.insert(id.to_string());
            let message = format!("\"{name}\": {}", lang::tx(&self.settings.language, &e.to_string()));
            self.toast(ToastKind::Error, message);
            return;
        }
        let path = entry.path.clone();
        let name = entry.name.clone();
        let gain = self.settings.default_sound_volume * entry.volume;
        self.missing.remove(id);
        self.engine
            .play(id, &path, gain, self.settings.retrigger, self.settings.allow_overlap);
        self.note_played(id, name);
    }

    /// Write a backup of the library, settings and history next to the data folder.
    fn export_backup(&self) -> Result<String, String> {
        let backup = serde_json::json!({
            "version": 1,
            "library": serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&self.paths.library_file).unwrap_or_else(|_| "{}".into())).unwrap_or(serde_json::Value::Null),
            "settings": serde_json::from_str::<serde_json::Value>(&std::fs::read_to_string(&self.paths.settings_file).unwrap_or_else(|_| "{}".into())).unwrap_or(serde_json::Value::Null),
            "history": self.history.iter().map(|h| serde_json::json!({"sound_id": h.sound_id, "name": h.name, "at": h.at})).collect::<Vec<_>>(),
        });
        let path = self.paths.root().join(format!("vibedrop-backup-{}.json", unix_now()));
        let text = serde_json::to_string_pretty(&backup).map_err(|e| e.to_string())?;
        crate::fsutil::write_atomic(&path, text.as_bytes()).map_err(|e| e.to_string())?;
        Ok(path.display().to_string())
    }

    /// Restore a backup file into the library and settings.
    fn import_backup(&mut self, path: &std::path::Path) -> Result<(), String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let data: serde_json::Value = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        if let Some(lib) = data.get("library") {
            let lib_text = serde_json::to_string(lib).map_err(|e| e.to_string())?;
            crate::fsutil::write_atomic(&self.paths.library_file, lib_text.as_bytes()).map_err(|e| e.to_string())?;
        }
        if let Some(set) = data.get("settings") {
            let set_text = serde_json::to_string(set).map_err(|e| e.to_string())?;
            crate::fsutil::write_atomic(&self.paths.settings_file, set_text.as_bytes()).map_err(|e| e.to_string())?;
        }
        if let Some(items) = data.get("history").and_then(|v| v.as_array()) {
            self.history.clear();
            for item in items {
                let (Some(sound_id), Some(name)) = (
                    item.get("sound_id").and_then(|v| v.as_str()),
                    item.get("name").and_then(|v| v.as_str()),
                ) else {
                    continue;
                };
                self.history.push_back(HistoryItem {
                    sound_id: sound_id.to_string(),
                    name: name.to_string(),
                    at: item.get("at").and_then(|v| v.as_u64()).unwrap_or_else(unix_now),
                });
            }
            self.history.truncate(HISTORY_MAX);
            self.save_history();
        }
        Ok(())
    }

    /// Open the data folder in the file manager.
    fn open_data_folder(&self) {
        #[cfg(target_os = "windows")]
        let _ = std::process::Command::new("explorer").arg(self.paths.root()).spawn();
        #[cfg(target_os = "linux")]
        let _ = std::process::Command::new("xdg-open").arg(self.paths.root()).spawn();
        #[cfg(target_os = "macos")]
        let _ = std::process::Command::new("open").arg(self.paths.root()).spawn();
    }

    fn note_played(&mut self, id: &str, name: String) {
        self.now_playing = Some(id.to_string());
        self.history.push_front(HistoryItem {
            sound_id: id.to_string(),
            name,
            at: unix_now(),
        });
        self.history.truncate(HISTORY_MAX);
        self.save_history();
    }

    /// Keep the "recently played" list between sessions.
    fn save_history(&self) {
        let items: Vec<serde_json::Value> = self
            .history
            .iter()
            .map(|h| serde_json::json!({"sound_id": h.sound_id, "name": h.name, "at": h.at}))
            .collect();
        if let Ok(text) = serde_json::to_string_pretty(&items) {
            let _ = crate::fsutil::write_atomic(&self.paths.history_file, text.as_bytes());
        }
    }

    fn load_history(&mut self) {
        let Ok(text) = std::fs::read_to_string(&self.paths.history_file) else {
            return;
        };
        let Ok(items) = serde_json::from_str::<Vec<serde_json::Value>>(&text) else {
            return;
        };
        for item in items {
            let (Some(sound_id), Some(name)) = (
                item.get("sound_id").and_then(|v| v.as_str()),
                item.get("name").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            self.history.push_back(HistoryItem {
                sound_id: sound_id.to_string(),
                name: name.to_string(),
                at: item.get("at").and_then(|v| v.as_u64()).unwrap_or_else(unix_now),
            });
        }
        self.history.truncate(HISTORY_MAX);
    }

    // ---- Library operations ------------------------------------------------------------------

    fn import_paths(&mut self, files: Vec<PathBuf>) {
        let lang = self.settings.language.clone();
        let managed = if self.settings.copy_on_import {
            Some(self.paths.sounds_dir.clone())
        } else {
            None
        };
        let (mut added, mut failed, mut duplicates) = (0usize, 0usize, 0usize);
        for file in files {
            match import_file(&file, managed.as_deref(), ImportSource::Local) {
                Ok(entry) => {
                    let duplicate = entry
                        .content_hash
                        .and_then(|h| self.library.find_by_hash(h))
                        .map(|e| e.name.clone());
                    if let Some(existing) = duplicate {
                        // Remove the copy that was just made (never the user's original file).
                        if let Err(e) = remove_managed_file(&entry, &self.paths.sounds_dir) {
                            tracing::warn!("could not remove the duplicate copy: {e}");
                        }
                        duplicates += 1;
                        if duplicates <= 3 {
                            let text = if lang::ru(&lang) {
                                format!("Уже есть в библиотеке как «{existing}», пропущено")
                            } else {
                                format!("Already in your library as \"{existing}\", skipped")
                            };
                            self.toast(ToastKind::Info, text);
                        }
                        continue;
                    }
                    self.engine.preload(&entry.id, &entry.path);
                    self.library.add_entry(entry);
                    added += 1;
                }
                Err(e) => {
                    failed += 1;
                    if failed <= 3 {
                        let name = file
                            .file_name()
                            .map(|n| n.to_string_lossy().into_owned())
                            .unwrap_or_default();
                        self.toast(ToastKind::Error, format!("{name}: {}", lang::tx(&lang, &e.to_string())));
                    }
                }
            }
        }
        if failed > 3 {
            let text = if lang::ru(&lang) {
                format!("Ещё файлов не удалось импортировать: {}", failed - 3)
            } else {
                format!("{} more files could not be imported", failed - 3)
            };
            self.toast(ToastKind::Error, text);
        }
        if added > 0 {
            self.save_library();
            self.sync_index();
            let msg = if added == 1 {
                lang::t(&lang, "Imported 1 sound").to_string()
            } else if lang::ru(&lang) {
                format!("Импортировано звуков: {added}")
            } else {
                format!("Imported {added} sounds")
            };
            self.toast(ToastKind::Success, msg);
        }
    }

    fn pick_and_import(&mut self) {
        #[cfg(target_os = "android")]
        {
            self.toast(
                ToastKind::Info,
                lang::t(
                    &self.settings.language,
                    "File import is not available on this platform yet.",
                ),
            );
            return;
        }
        #[cfg(not(target_os = "android"))]
        {
            let picked = rfd::FileDialog::new()
                .set_title(lang::t(&self.settings.language, "Import sounds"))
                .add_filter(
                    lang::t(&self.settings.language, "Audio & video (WAV, MP3, MP4)"),
                    &["wav", "mp3", "mp4", "m4a", "mov"],
                )
                .pick_files();
            if let Some(files) = picked {
                self.import_paths(files);
            }
        }
    }

    fn remove_sound(&mut self, id: &str) {
        let lang = self.settings.language.clone();
        self.engine.stop(id);
        if let Some(hk) = self.hotkeys.as_mut() {
            hk.unregister(id);
        }
        match self.library.remove(id) {
            Ok(entry) => {
                if self.settings.delete_managed_copy_on_remove {
                    if let Err(e) = remove_managed_file(&entry, &self.paths.sounds_dir) {
                        let text = if lang::ru(&lang) {
                            format!("Не удалось удалить файл: {}", lang::tx(&lang, &e.to_string()))
                        } else {
                            format!("Could not delete the file: {e}")
                        };
                        self.toast(ToastKind::Error, text);
                    }
                }
                if !self.library.entries.iter().any(|e| e.path == entry.path) {
                    self.engine.forget_file(&entry.path);
                }
                self.missing.remove(id);
                self.inactive_hotkeys.remove(id);
                if self.now_playing.as_deref() == Some(id) {
                    self.now_playing = None;
                }
                self.save_library();
                self.sync_index();
                let text = if lang::ru(&lang) {
                    format!("Удалено из библиотеки: «{}»", entry.name)
                } else {
                    format!("Removed \"{}\" from the library", entry.name)
                };
                self.toast(ToastKind::Info, text);
            }
            Err(e) => self.toast(ToastKind::Error, lang::tx(&lang, &e.to_string())),
        }
    }

    /// Re-scan the library for files that disappeared and report the result.
    fn check_library_files(&mut self) {
        let lang = self.settings.language.clone();
        let missing = self.library.missing_ids();
        let count = missing.len();
        self.missing = missing.into_iter().collect();
        if count == 0 {
            let text = if lang::ru(&lang) {
                format!("Все файлы звуков на месте ({})", self.library.len())
            } else {
                format!("All {} sound files are present", self.library.len())
            };
            self.toast(ToastKind::Success, text);
        } else {
            let text = if lang::ru(&lang) {
                format!("Отсутствуют файлы звуков: {count}. Нажмите «Указать файл...» в меню \u{2022}\u{2022}\u{2022}, чтобы найти их.")
            } else {
                format!("{count} sound file(s) are missing. Use \"Locate file...\" in the \u{2022}\u{2022}\u{2022} menu to repair them.")
            };
            self.toast(ToastKind::Error, text);
        }
    }

    /// Let the user point a library entry at a new file (for example after moving it).
    fn relink_sound(&mut self, id: &str) {
        #[cfg(target_os = "android")]
        {
            let _ = id;
            self.toast(
                ToastKind::Info,
                lang::t(
                    &self.settings.language,
                    "File import is not available on this platform yet.",
                ),
            );
            return;
        }
        #[cfg(not(target_os = "android"))]
        {
            let Some(picked) = rfd::FileDialog::new()
                .set_title(lang::t(&self.settings.language, "Locate the sound file"))
                .add_filter(
                    lang::t(&self.settings.language, "Audio & video (WAV, MP3, MP4)"),
                    &["wav", "mp3", "mp4", "m4a", "mov"],
                )
                .pick_file()
            else {
                return;
            };
            if let Err(e) = validate_audio_file(&picked) {
                self.toast(ToastKind::Error, lang::tx(&self.settings.language, &e.to_string()));
                return;
            }
            let hash = file_fingerprint(&picked).ok();
            if let Some(e) = self.library.get_mut(id) {
                e.path = picked.clone();
                e.duration_secs = None;
                if hash.is_some() {
                    e.content_hash = hash;
                }
            }
            self.missing.remove(id);
            self.engine.preload(id, &picked);
            self.save_library();
            self.sync_index();
            self.toast(ToastKind::Success, lang::t(&self.settings.language, "File linked"));
        }
    }

    /// Decode a video file (MP4/MOV) and save its audio track as a new WAV library entry.
    fn extract_audio(&mut self, id: &str) {
        let Some(entry) = self.library.get(id).cloned() else {
            return;
        };
        let lang = self.settings.language.clone();
        match self.extract_audio_inner(&entry) {
            Ok(name) => {
                let text = if lang::ru(&lang) {
                    format!("Звук извлечён: {name}")
                } else {
                    format!("Audio extracted: {name}")
                };
                self.toast(ToastKind::Success, text);
            }
            Err(e) => self.toast(ToastKind::Error, lang::tx(&lang, &e)),
        }
    }

    fn extract_audio_inner(&mut self, entry: &SoundEntry) -> Result<String, String> {
        let decoded = crate::audio::engine::decode_file(&entry.path)?;
        let stamp = unix_now();
        std::fs::create_dir_all(&self.paths.sounds_dir).map_err(|e| e.to_string())?;
        let dest = self.paths.sounds_dir.join(format!("{}-{stamp}.wav", entry.id));
        write_wav16(&dest, &decoded.samples, decoded.rate).map_err(|e| e.to_string())?;
        let mut sound = import_file(&dest, None, ImportSource::Local).map_err(|e| e.to_string())?;
        sound.name = format!("{} (audio)", entry.name);
        sound.duration_secs = Some(decoded.duration_secs());
        let name = sound.name.clone();
        self.library.add_entry(sound);
        self.library_dirty = true;
        self.sync_index();
        Ok(name)
    }

    /// Hotkeys of sounds and the microphone-mute hotkey share one namespace.
    fn apply_hotkey(&mut self, id: &str, hotkey: Option<&str>) -> Result<(), String> {
        if id == MIC_TOGGLE_ID {
            self.assign_mic_hotkey(hotkey)
        } else {
            self.assign_hotkey(id, hotkey)
        }
    }

    fn assign_mic_hotkey(&mut self, hotkey: Option<&str>) -> Result<(), String> {
        let new = match hotkey {
            Some(h) => Some(normalize_hotkey(h).map_err(|e| e.to_string())?),
            None => None,
        };
        if let Some(h) = &new {
            if let Some(owner) = self.library.find_hotkey_conflict(h, None) {
                if lang::ru(&self.settings.language) {
                    return Err(format!("Сочетание {h} уже используется звуком «{}»", owner.name));
                }
                return Err(format!("{h} is already used by \"{}\"", owner.name));
            }
        }
        if let Some(hk) = self.hotkeys.as_mut() {
            match &new {
                Some(h) => hk.register(MIC_TOGGLE_ID, h).map_err(|e| e.to_string())?,
                None => hk.unregister(MIC_TOGGLE_ID),
            }
        } else if new.is_some() {
            self.toast(
                ToastKind::Error,
                lang::t(
                    &self.settings.language,
                    "Global hotkeys are unavailable, so this hotkey is saved but inactive.",
                ),
            );
        }
        self.settings.mic_mute_hotkey = new;
        self.mic_hotkey_inactive = false;
        self.settings_dirty = true;
        Ok(())
    }

    /// Set or clear the hotkey of a sound. The library and the OS registration are kept consistent:
    /// if the OS refuses the combination, the previous assignment is restored.
    fn assign_hotkey(&mut self, id: &str, hotkey: Option<&str>) -> Result<(), String> {
        if let Some(Ok(n)) = hotkey.map(normalize_hotkey) {
            if self.settings.mic_mute_hotkey.as_deref() == Some(n.as_str()) {
                if lang::ru(&self.settings.language) {
                    return Err(format!(
                        "Сочетание {n} уже используется для отключения звука маршрута микрофона"
                    ));
                }
                return Err(format!("{n} is already used for muting the microphone route"));
            }
        }
        let old = self.library.get(id).and_then(|e| e.hotkey.clone());
        let new = self.library.set_hotkey(id, hotkey).map_err(|e| e.to_string())?;
        if let Some(hk) = self.hotkeys.as_mut() {
            let result = match &new {
                Some(h) => hk.register(id, h).map_err(|e| e.to_string()),
                None => {
                    hk.unregister(id);
                    Ok(())
                }
            };
            if let Err(message) = result {
                let _ = self.library.set_hotkey(id, old.as_deref());
                return Err(message);
            }
        } else if new.is_some() {
            self.toast(
                ToastKind::Error,
                lang::t(
                    &self.settings.language,
                    "Global hotkeys are unavailable, so this hotkey is saved but inactive.",
                ),
            );
        }
        self.inactive_hotkeys.remove(id);
        self.save_library();
        Ok(())
    }

    // ---- Downloads and links -----------------------------------------------------------------

    fn parse_target(text: &str) -> Result<Url, String> {
        let t = text.trim();
        if t.is_empty() {
            return Err("Paste a link first".to_string());
        }
        parse_url_text(t).map_err(|e| e.to_string())
    }
    fn start_download(&mut self, url: Url, play_after: bool) -> Result<(), String> {
        downloader::validate_url(&url).map_err(|e| e.to_string())?;
        let id = self.next_download;
        self.next_download += 1;
        let label = downloader::suggested_name(&url);
        let url_copy = url.clone();
        let handle = downloader::spawn(id, url, self.paths.sounds_dir.clone(), self.dl_tx.clone());
        self.downloads.insert(
            id,
            ActiveDownload {
                handle,
                received: 0,
                total: None,
                label,
                play_after,
                url: url_copy,
            },
        );
        Ok(())
    }

    fn handle_link(&mut self, link: &str) {
        let lang = self.settings.language.clone();
        match parse_uri(link) {
            Ok(ProtocolCommand::Play { url }) => {
                let text = match url.host_str() {
                    Some(host) if lang::ru(&lang) => format!("Скачиваем звук с {host}..."),
                    Some(host) => format!("Downloading a sound from {host}..."),
                    None if lang::ru(&lang) => "Скачиваем звук из интернета...".to_string(),
                    None => "Downloading a sound from the internet...".to_string(),
                };
                match self.start_download(url, true) {
                    Ok(()) => self.toast(ToastKind::Info, text),
                    Err(e) => self.toast(ToastKind::Error, lang::tx(&lang, &e)),
                }
            }
            Err(e) => self.toast(ToastKind::Error, lang::tx(&lang, &e.to_string())),
        }
    }

    // ---- Event pumping -----------------------------------------------------------------------

    fn pump_events(&mut self, ctx: &egui::Context) {
        let lang = self.settings.language.clone();
        let ru = lang::ru(&lang);
        for event in self.engine.poll_events() {
            match event {
                EngineEvent::Decoded {
                    sound_id,
                    duration_secs,
                } => {
                    let changed = self
                        .library
                        .get(&sound_id)
                        .is_some_and(|e| e.duration_secs.is_none_or(|d| (d - duration_secs).abs() > 0.01));
                    if changed {
                        self.library.set_duration(&sound_id, duration_secs);
                        self.library_dirty = true;
                    }
                }
                EngineEvent::Error { sound_id, message } => {
                    let message = lang::tx(&lang, &message);
                    let text = match sound_id.as_deref().and_then(|id| self.library.get(id)) {
                        Some(entry) => format!("\"{}\": {message}", entry.name),
                        None => message,
                    };
                    self.toast(ToastKind::Error, text);
                }
                EngineEvent::DeviceLost { role, device, reason } => {
                    let reason = lang::tx(&lang, &reason);
                    let text = if ru {
                        format!(
                            "{}: устройство «{device}» перестало работать ({reason}). VibeDrop пытается восстановить связь.",
                            lang::t(&lang, role.label())
                        )
                    } else {
                        format!(
                            "{}: \"{device}\" stopped working ({reason}). VibeDrop is trying to reconnect.",
                            role.label()
                        )
                    };
                    self.toast(ToastKind::Error, text);
                }
                EngineEvent::Recovered { role, device } => {
                    let text = if ru {
                        format!("{}: «{device}» снова работает", lang::t(&lang, role.label()))
                    } else {
                        format!("{}: \"{device}\" is working again", role.label())
                    };
                    self.toast(ToastKind::Success, text);
                }
                EngineEvent::FellBack { role, wanted, using } => {
                    let text = if ru {
                        format!(
                            "{}: «{wanted}» недоступно. Звук играет через «{using}».",
                            lang::t(&lang, role.label())
                        )
                    } else {
                        format!(
                            "{}: \"{wanted}\" is not available. Playing on \"{using}\" instead.",
                            role.label()
                        )
                    };
                    self.toast(ToastKind::Error, text);
                }
                EngineEvent::VoiceLost { device, reason } => {
                    let reason = lang::tx(&lang, &reason);
                    let text = if ru {
                        format!("Ваш микрофон: устройство «{device}» перестало работать ({reason}). VibeDrop пытается восстановить связь.")
                    } else {
                        format!(
                            "Your microphone: \"{device}\" stopped working ({reason}). VibeDrop is trying to reconnect."
                        )
                    };
                    self.toast(ToastKind::Error, text);
                }
                EngineEvent::VoiceRecovered { device } => {
                    let text = if ru {
                        format!("Ваш микрофон: «{device}» снова работает")
                    } else {
                        format!("Your microphone: \"{device}\" is working again")
                    };
                    self.toast(ToastKind::Success, text);
                }
                EngineEvent::RouteTest(report) => {
                    self.route_test_running = false;
                    let kind = match report.level {
                        ReportLevel::Ok => ToastKind::Success,
                        ReportLevel::Warning | ReportLevel::Failure => ToastKind::Error,
                    };
                    self.toast(
                        kind,
                        if report.level == ReportLevel::Ok {
                            lang::t(&lang, "Microphone route works")
                        } else {
                            lang::t(&lang, "Microphone route test: see Settings")
                        },
                    );
                    self.route_test = Some(report);
                }
            }
        }

        while let Ok(event) = self.dl_rx.try_recv() {
            match event {
                DownloadEvent::Progress { id, received, total } => {
                    if let Some(dl) = self.downloads.get_mut(&id) {
                        dl.received = received;
                        dl.total = total;
                    }
                }
                DownloadEvent::Finished { id, result } => {
                    let dl = self.downloads.remove(&id);
                    let play_after = dl.as_ref().is_some_and(|d| d.play_after);
                    match result {
                        Ok(file) => self.finish_download(file, play_after),
                        Err(DownloadError::Cancelled) => {
                            self.toast(ToastKind::Info, lang::t(&lang, "Download cancelled"))
                        }
                        Err(e) => {
                            let message = lang::tx(&lang, &e.to_string());
                            let text = if ru {
                                format!("Не удалось скачать: {message}")
                            } else {
                                format!("Download failed: {message}")
                            };
                            self.toast(ToastKind::Error, text);
                            if let Some(d) = dl {
                                self.failed_downloads.push(FailedDownload {
                                    label: d.label,
                                    url: d.url,
                                    error: message,
                                    play_after: d.play_after,
                                });
                                if self.failed_downloads.len() > MAX_FAILED_DOWNLOADS {
                                    self.failed_downloads.remove(0);
                                }
                            }
                        }
                    }
                }
            }
        }

        while let Ok(msg) = self.helper_rx.try_recv() {
            match msg {
                HelperMsg::Cable(result) => match result {
                    Ok(()) => self.toast(
                        ToastKind::Success,
                        lang::t(&lang, "VB-CABLE downloaded: run VBCABLE_Setup_x64.exe as administrator, then select CABLE Input in Audio devices."),
                    ),
                    Err(e) => self.toast(ToastKind::Error, e),
                },
                HelperMsg::CableProgress(received, total) => {
                    if let Some(setup) = self.setup.as_mut() {
                        setup.cable_progress = if total > 0 {
                            (received as f32 / total as f64 as f32).clamp(0.0, 1.0)
                        } else {
                            0.0
                        };
                    }
                }
                HelperMsg::CableReady(path) => {
                    if let Some(setup) = self.setup.as_mut() {
                        setup.cable_done = true;
                        setup.busy = false;
                        setup.cable_status = path.clone();
                    }
                    self.toast(ToastKind::Success, lang::t(&lang, "VB-CABLE downloaded"));
                }
            }
        }

        while let Ok(id) = self.hk_rx.try_recv() {
            if id == MIC_TOGGLE_ID {
                let muted = self.engine.levels().mic_muted;
                self.settings.mic_muted = muted;
                self.settings_dirty = true;
                let text = if muted {
                    lang::t(&lang, "Microphone route muted")
                } else {
                    lang::t(&lang, "Microphone route unmuted")
                };
                self.toast(ToastKind::Info, text);
            } else if let Some(name) = self.library.get(&id).map(|e| e.name.clone()) {
                self.note_played(&id, name);
            }
        }

        while let Ok(line) = self.ipc_rx.try_recv() {
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            if !line.eq_ignore_ascii_case(system::SHOW_MESSAGE) {
                self.handle_link(&line);
            }
        }

        // Do not rewrite the file on every frame while a slider is being dragged.
        if self.library_dirty && !ctx.input(|i| i.pointer.any_down()) {
            self.save_library();
        }
    }

    /// A download finished successfully: add it to the library (or reuse an identical sound).
    fn finish_download(&mut self, file: downloader::DownloadedFile, play_after: bool) {
        let hash = file_fingerprint(&file.path).ok();
        let existing = hash
            .and_then(|h| self.library.find_by_hash(h))
            .map(|e| (e.id.clone(), e.name.clone()));
        if let Some((existing_id, existing_name)) = existing {
            if let Err(e) = std::fs::remove_file(&file.path) {
                tracing::warn!("could not remove the duplicate download: {e}");
            }
            let text = if lang::ru(&self.settings.language) {
                format!("Уже есть в библиотеке как «{existing_name}»")
            } else {
                format!("Already in your library as \"{existing_name}\"")
            };
            self.toast(ToastKind::Info, text);
            if play_after {
                self.play_sound(&existing_id);
            }
            return;
        }
        let name = if file.suggested_name.trim().is_empty() {
            lang::t(&self.settings.language, "Sound").to_string()
        } else {
            file.suggested_name.clone()
        };
        let entry = SoundEntry {
            id: uuid::Uuid::new_v4().to_string(),
            name: name.clone(),
            path: file.path.clone(),
            duration_secs: None,
            source: ImportSource::Url(file.source_url.clone()),
            hotkey: None,
            favorite: false,
            added_at: unix_now(),
            volume: 1.0,
            content_hash: hash,
        };
        let sound_id = entry.id.clone();
        self.engine.preload(&sound_id, &entry.path);
        self.library.add_entry(entry);
        self.save_library();
        self.sync_index();
        let text = if lang::ru(&self.settings.language) {
            format!("Добавлено: «{name}»")
        } else {
            format!("Added \"{name}\"")
        };
        self.toast(ToastKind::Success, text);
        if play_after {
            self.play_sound(&sound_id);
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| i.raw.dropped_files.iter().filter_map(|f| f.path.clone()).collect());
        if !dropped.is_empty() {
            self.import_paths(dropped);
        }
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            let painter = ctx.layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("drop_overlay"),
            ));
            let screen = ctx.screen_rect();
            painter.rect_filled(screen, 0.0, Color32::from_black_alpha(190));
            painter.rect_stroke(screen.shrink(18.0), 16.0, Stroke::new(2.0_f32, theme::c_accent()));
            let text = if lang::ru(&self.settings.language) {
                "Перетащите файлы WAV / MP3 сюда для импорта"
            } else {
                "Drop WAV / MP3 / MP4 files to import"
            };
            painter.text(
                screen.center(),
                Align2::CENTER_CENTER,
                text,
                egui::FontId::proportional(24.0),
                theme::c_accent(),
            );
        }
    }

    fn handle_close(&mut self, ctx: &egui::Context) {
        let close_requested = ctx.input(|i| i.viewport().close_requested());
        if close_requested && !self.quitting && self.settings.close_behavior == CloseBehavior::Minimize {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(true));
        }
    }

    // ---- Drawing: panels ---------------------------------------------------------------------

    fn draw_sidebar(&mut self, ctx: &egui::Context) {
        let lang = self.settings.language.clone();
        // Compact phone layout: narrow windows get a horizontal top strip instead of the rail.
        let narrow = ctx.input(|i| i.screen_rect.width()) < 700.0;
        if narrow {
            let mut go_page: Option<Page> = None;
            let mut want_quit = false;
            egui::TopBottomPanel::top("categories-top")
                .resizable(false)
                .frame(
                    egui::Frame::none()
                        .fill(theme::c_sidebar())
                        .stroke(Stroke::new(1.0_f32, theme::c_border()))
                        .inner_margin(egui::Margin::symmetric(8.0, 6.0)),
                )
                .show(ctx, |ui| {
                    egui::ScrollArea::horizontal()
                        .auto_shrink([true, false])
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                let (mark, _) = ui.allocate_exact_size(vec2(36.0, 36.0), Sense::hover());
                                theme::brand_mark(ui.painter(), mark);
                                for page in Page::ALL {
                                    let label = if page == Page::Library {
                                        lang::t(&lang, "My sounds")
                                    } else {
                                        lang::t(&lang, page.title())
                                    };
                                    if theme::rail_button(ui, page.icon(), label, self.page == page)
                                        .on_hover_text(lang::t(&lang, page.title()))
                                        .clicked()
                                    {
                                        go_page = Some(page);
                                    }
                                }
                                if theme::icon_button(ui, Icon::Quit, 17.0, false)
                                    .on_hover_text(lang::t(&lang, "Quit VibeDrop"))
                                    .clicked()
                                {
                                    want_quit = true;
                                }
                            });
                        });
                });
            if let Some(page) = go_page {
                self.page = page;
                if page == Page::Settings {
                    self.protocol_state = system::protocol_state();
                }
            }
            if want_quit {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
            return;
        }
        egui::SidePanel::left("sidebar")
            .exact_width(196.0)
            .resizable(false)
            .frame(
                egui::Frame::none()
                    .fill(theme::c_sidebar())
                    .stroke(Stroke::new(1.0_f32, theme::c_border()))
                    .inner_margin(egui::Margin::symmetric(12.0, 14.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    let (mark, _) = ui.allocate_exact_size(vec2(38.0, 38.0), Sense::hover());
                    theme::brand_mark(ui.painter(), mark);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 0.0;
                            ui.label(RichText::new("Vibe").size(18.0).strong().color(theme::c_text()));
                            ui.label(RichText::new("Drop").size(18.0).strong().color(theme::c_accent()));
                        });
                        ui.label(
                            RichText::new("SOUNDBOARD")
                                .size(9.0)
                                .strong()
                                .color(theme::c_text_faint()),
                        );
                    });
                });
                ui.add_space(12.0);
                ui.label(
                    RichText::new(lang::t(&lang, "CATEGORY"))
                        .size(10.5)
                        .strong()
                        .color(theme::c_text_faint()),
                );
                ui.add_space(3.0);
                for page in Page::ALL {
                    let label = if page == Page::Library {
                        lang::t(&lang, "My sounds")
                    } else {
                        lang::t(&lang, page.title())
                    };
                    let icon = if page == Page::Library { Icon::Home } else { page.icon() };
                    if theme::nav_item(
                        ui,
                        icon,
                        label,
                        self.page == page,
                        self.settings.theme.show_sidebar_labels,
                    )
                    .clicked()
                    {
                        self.page = page;
                        if page == Page::Settings {
                            self.protocol_state = system::protocol_state();
                        }
                    }
                }
                ui.add_space((ui.available_height() - 60.0).max(0.0));
                let favorites = self.library.favorites().len();
                let counts = if lang::ru(&lang) {
                    format!("{} звуков · {} избранных", self.library.len(), favorites)
                } else {
                    format!("{} sounds · {} favorites", self.library.len(), favorites)
                };
                ui.label(RichText::new(counts).size(12.0).color(theme::c_text_faint()));
                if theme::nav_item(
                    ui,
                    Icon::Quit,
                    lang::t(&lang, "Quit VibeDrop"),
                    false,
                    self.settings.theme.show_sidebar_labels,
                )
                .clicked()
                {
                    self.quitting = true;
                    ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                }
            });
    }

    /// The menu bar: one compact row of drop-downs, in the classic soundboard style.
    fn draw_menubar(&mut self, ctx: &egui::Context) {
        let lang = self.settings.language.clone();
        let mut want_import = false;
        let mut want_url = false;
        let mut want_check = false;
        let mut want_quit = false;
        let mut want_about = false;
        let mut want_cable_page = false;
        let mut want_setup = false;
        let mut go_page: Option<Page> = None;
        let mut stop_all = false;

        egui::TopBottomPanel::top("menubar")
            .exact_height(28.0)
            .frame(
                egui::Frame::none()
                    .fill(theme::c_sidebar())
                    .inner_margin(egui::Margin::symmetric(6.0, 2.0)),
            )
            .show(ctx, |ui| {
                // Flat menu items: no button frame, just the text (classic soundpad look).
                ui.style_mut().visuals.widgets.inactive.bg_fill = Color32::TRANSPARENT;
                ui.style_mut().visuals.widgets.inactive.bg_stroke = Stroke::NONE;
                ui.style_mut().visuals.widgets.inactive.weak_bg_fill = Color32::TRANSPARENT;
                ui.style_mut().visuals.widgets.hovered.bg_fill = theme::c_card_hover();
                ui.style_mut().visuals.widgets.hovered.bg_stroke = Stroke::NONE;
                ui.style_mut().visuals.widgets.open.bg_fill = theme::c_card_hover();
                ui.style_mut().visuals.widgets.open.bg_stroke = Stroke::NONE;
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;

                    // Р¤Р°Р№Р»
                    ui.menu_button(lang::t(&lang, "File"), |ui| {
                        if ui.button(lang::t(&lang, "Import")).clicked() {
                            want_import = true;
                            ui.close_menu();
                        }
                        if ui.button(lang::t(&lang, "Add from URL")).clicked() {
                            want_url = true;
                            ui.close_menu();
                        }
                        if ui.button(lang::t(&lang, "Check files")).clicked() {
                            want_check = true;
                            ui.close_menu();
                        }
                        if ui.button(lang::t(&lang, "Setup")).clicked() {
                            want_setup = true;
                            ui.close_menu();
                        }
                        ui.separator();
                        if ui.button(lang::t(&lang, "Quit VibeDrop")).clicked() {
                            want_quit = true;
                            ui.close_menu();
                        }
                    });
                    // Р РµРґР°РєС‚РёСЂРѕРІР°РЅРёРµ
                    ui.menu_button(lang::t(&lang, "Edit"), |ui| {
                        if ui.button(lang::t(&lang, "Import")).clicked() {
                            want_import = true;
                            ui.close_menu();
                        }
                        if ui.button(lang::t(&lang, "Check files")).clicked() {
                            want_check = true;
                            ui.close_menu();
                        }
                    });
                    // Р’РѕСЃРїСЂРѕРёР·РІРµРґРµРЅРёРµ
                    ui.menu_button(lang::t(&lang, "Playback"), |ui| {
                        if ui.button(lang::t(&lang, "Stop All")).clicked() {
                            stop_all = true;
                            ui.close_menu();
                        }
                        if ui.button(lang::t(&lang, "Test sound")).clicked() {
                            self.engine.test_tone(OutputRole::Main);
                            ui.close_menu();
                        }
                    });
                    // Р’РёРґ
                    ui.menu_button(lang::t(&lang, "Window"), |ui| {
                        for page in Page::ALL {
                            if ui.button(lang::t(&lang, page.title())).clicked() {
                                go_page = Some(page);
                                ui.close_menu();
                            }
                        }
                    });
                    // РЎРїСЂР°РІРєР°
                    ui.menu_button(lang::t(&lang, "Help"), |ui| {
                        if ui.button(lang::t(&lang, "Official VB-CABLE page")).clicked() {
                            want_cable_page = true;
                            ui.close_menu();
                        }
                        if ui.button(lang::t(&lang, "About VibeDrop")).clicked() {
                            want_about = true;
                            ui.close_menu();
                        }
                    });
                });
            });

        if want_import {
            self.pick_and_import();
        }
        if want_url {
            self.url_dialog = Some(UrlDialog {
                text: String::new(),
                error: None,
                focus: true,
            });
        }
        if want_check {
            self.check_library_files();
        }
        if let Some(page) = go_page {
            self.page = page;
            if page == Page::Settings {
                self.protocol_state = system::protocol_state();
            }
        }
        if want_cable_page {
            ctx.open_url(egui::OpenUrl::new_tab("https://vb-audio.com/Cable/"));
        }
        if want_setup {
            self.setup = Some(SetupState::default());
        }
        if want_about {
            self.about = true;
        }
        if stop_all {
            self.engine.stop_all();
        }
        if want_quit {
            self.quitting = true;
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }
    }
    /// The transport row: play/stop, time, a seekable progress bar and the master volume.
    fn draw_transport(&mut self, ctx: &egui::Context) {
        let lang = self.settings.language.clone();
        let now_id = self.now_playing.clone();
        let playing = now_id.as_deref().is_some_and(|id| self.engine.is_playing(id));
        let progress = now_id.as_deref().and_then(|id| self.engine.progress(id));
        let mut toggle = false;
        let mut levels_changed = false;
        let mut seek: Option<f32> = None;
        if self.settings.theme.show_transport {
            egui::TopBottomPanel::top("transport")
                .exact_height(42.0)
                .frame(
                    egui::Frame::none()
                        .fill(theme::c_sidebar())
                        .stroke(Stroke::new(1.0_f32, theme::c_border()))
                        .inner_margin(egui::Margin::symmetric(10.0, 5.0)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 6.0;
                        let icon = if playing { Icon::Stop } else { Icon::Play };
                        if theme::toolbar_button(ui, icon, lang::t(&lang, "Play")).clicked() {
                            toggle = true;
                        }
                        let time_text = match progress {
                            Some((pos, total)) => format!("{} / {}", theme::fmt_time(pos), theme::fmt_time(total)),
                            None => "0:00".to_string(),
                        };
                        ui.label(
                            RichText::new(time_text)
                                .monospace()
                                .size(12.5)
                                .color(theme::c_text_dim()),
                        );
                        let bar_w = (ui.available_width() - 210.0).max(120.0);
                        let (rect, resp) = ui.allocate_exact_size(vec2(bar_w, 6.0), Sense::click_and_drag());
                        ui.painter().rect_filled(rect, 3.0, theme::c_border());
                        let frac = match progress {
                            Some((pos, total)) if total > 0.0 => (pos / total).clamp(0.0, 1.0) as f32,
                            _ => 0.0,
                        };
                        let filled = egui::Rect::from_min_size(rect.min, vec2(bar_w * frac, 6.0));
                        ui.painter().rect_filled(filled, 3.0, theme::c_accent());
                        if progress.is_some() && (resp.clicked() || resp.dragged()) {
                            if let Some(pos) = resp.interact_pointer_pos() {
                                let f = ((pos.x - rect.min.x) / rect.width()).clamp(0.0, 1.0);
                                seek = Some(f);
                            }
                        }
                        let (icon_rect, _) = ui.allocate_exact_size(vec2(20.0, 20.0), Sense::hover());
                        theme::paint_icon(
                            ui.painter(),
                            Icon::Headphones,
                            icon_rect.center(),
                            18.0,
                            theme::c_accent(),
                        );
                        ui.spacing_mut().slider_width = 120.0;
                        levels_changed |=
                            volume_row_inline(ui, lang::t(&lang, "Master"), &mut self.settings.master_volume, true);
                    });
                });
        }
        if levels_changed {
            self.engine.set_levels(self.levels());
            self.settings_dirty = true;
        }
        if let Some(frac) = seek {
            if let Some(id) = &now_id {
                self.engine.seek(id, frac);
            }
        }
        if toggle {
            if let Some(id) = now_id {
                if playing {
                    self.engine.stop(&id);
                } else {
                    self.play_sound(&id);
                }
            }
        }
    }
    /// The bottom status strip: what is selected, counters and the quick actions.
    fn draw_bottombar(&mut self, ctx: &egui::Context) {
        let lang = self.settings.language.clone();
        let mic_state = self.engine.route_state(OutputRole::Secondary);
        let mut open_settings = false;
        let mut stop_all = false;
        let mut mic_click = false;
        if self.settings.theme.show_status_bar {
            egui::TopBottomPanel::bottom("infobar")
                .exact_height(44.0)
                .frame(
                    egui::Frame::none()
                        .fill(theme::c_sidebar())
                        .stroke(Stroke::new(1.0_f32, theme::c_border()))
                        .inner_margin(egui::Margin::symmetric(12.0, 7.0)),
                )
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 10.0;
                        let (icon_rect, _) = ui.allocate_exact_size(vec2(26.0, 26.0), Sense::hover());
                        theme::paint_icon(ui.painter(), Icon::Home, icon_rect.center(), 22.0, theme::c_accent());
                        ui.label(
                            RichText::new(lang::t(&lang, self.page.title()))
                                .size(14.0)
                                .strong()
                                .color(theme::c_text()),
                        );
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(format!("{}: {}", lang::t(&lang, "Sounds"), self.library.len()))
                                .size(12.5)
                                .color(theme::c_text_dim()),
                        );
                        ui.label(
                            RichText::new(format!("{}: {}", lang::t(&lang, "Plays"), self.history.len()))
                                .size(12.5)
                                .color(theme::c_text_dim()),
                        );
                        if self.discord.status() == crate::discord::DiscordStatus::Connected {
                            ui.label(RichText::new("Discord").size(12.5).color(theme::c_accent()));
                        }
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if theme::toolbar_button(ui, Icon::Wrench, lang::t(&lang, "Settings")).clicked() {
                                open_settings = true;
                            }
                            if theme::toolbar_button(ui, Icon::StopAll, lang::t(&lang, "Stop All")).clicked() {
                                stop_all = true;
                            }
                            let (text, color, filled, tip) = if !self.settings.secondary_enabled {
                                (
                                    lang::t(&lang, "MIC OFF"),
                                    theme::c_text_dim(),
                                    false,
                                    lang::t(&lang, "The microphone route is off. Click to set it up."),
                                )
                            } else if mic_state.is_active() {
                                if self.settings.mic_muted {
                                    (
                                        lang::t(&lang, "MIC MUTED"),
                                        theme::c_warning(),
                                        false,
                                        lang::t(&lang, "Sounds are not sent to the microphone. Click to unmute."),
                                    )
                                } else {
                                    (
                                        lang::t(&lang, "MIC ON"),
                                        theme::c_accent(),
                                        true,
                                        lang::t(&lang, "Sounds are sent to the microphone route. Click to mute it."),
                                    )
                                }
                            } else {
                                (
                                    lang::t(&lang, "MIC !"),
                                    theme::c_danger(),
                                    false,
                                    lang::t(&lang, "The microphone route has a problem. Click to open Settings."),
                                )
                            };
                            if theme::chip(ui, text, color, filled).on_hover_text(tip).clicked() {
                                mic_click = true;
                            }
                        });
                    });
                });
        }
        if mic_click {
            if self.settings.secondary_enabled && mic_state.is_active() {
                self.toggle_mic_mute();
            } else {
                self.page = Page::Settings;
            }
        }
        if open_settings {
            self.page = Page::Settings;
        }
        if stop_all {
            self.engine.stop_all();
        }
    }
    /// A banner under the window top when an audio device needs attention. A silent failure is
    /// the worst outcome, so everything that is not "working as chosen" is shown here.
    fn draw_alert_banner(&mut self, ui: &mut Ui) {
        let lang = self.settings.language.clone();
        let mut lines: Vec<(String, bool)> = Vec::new();
        let main = self.engine.route_state(OutputRole::Main);
        match &main {
            RouteState::Active {
                fallback_from: None, ..
            } => {}
            RouteState::Disabled => {}
            other => lines.push((
                format!("{}: {}", lang::t(&lang, "Headphones"), describe_state(&lang, other)),
                !matches!(other, RouteState::Active { .. }),
            )),
        }
        if self.settings.secondary_enabled {
            match self.engine.route_state(OutputRole::Secondary) {
                RouteState::Active { .. } | RouteState::Disabled => {}
                other => lines.push((
                    format!(
                        "{}: {}",
                        lang::t(&lang, "Microphone route"),
                        describe_state(&lang, &other)
                    ),
                    true,
                )),
            }
            if self.settings.voice_enabled {
                match self.engine.voice_state() {
                    RouteState::Active { .. } | RouteState::Disabled | RouteState::Standby { .. } => {}
                    other => lines.push((
                        format!(
                            "{}: {}",
                            lang::t(&lang, "Your microphone"),
                            describe_state(&lang, &other)
                        ),
                        true,
                    )),
                }
            }
        }
        if lines.is_empty() {
            return;
        }
        let mut open_settings = false;
        theme::frame_card().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.vertical(|ui| {
                    for (text, severe) in &lines {
                        let color = if *severe { theme::c_danger() } else { theme::c_warning() };
                        ui.label(RichText::new(format!("\u{25CF} {text}")).size(13.0).color(color));
                    }
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if theme::secondary_button(ui, lang::t(&lang, "Audio settings")).clicked() {
                        open_settings = true;
                    }
                });
            });
        });
        ui.add_space(10.0);
        if open_settings {
            self.page = Page::Settings;
        }
    }

    fn draw_central(&mut self, ctx: &egui::Context) {
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(theme::c_bg())
                    .inner_margin(egui::Margin::symmetric(26.0, 20.0)),
            )
            .show(ctx, |ui| {
                self.draw_alert_banner(ui);
                match self.page {
                    Page::Library | Page::Favorites => self.draw_sound_page(ui),
                    Page::History => self.draw_history_page(ui),
                    Page::Settings => self.draw_settings_page(ui),
                }
            });
    }

    // ---- Drawing: sound list -----------------------------------------------------------------

    fn draw_sound_page(&mut self, ui: &mut Ui) {
        let lang = self.settings.language.clone();
        let favorites_only = self.page == Page::Favorites;
        let mut want_import = false;
        let mut want_url = false;
        let mut want_check = false;

        let favorites = self.library.favorites().len();
        let total = self.library.len();
        let (title, subtitle) = if favorites_only {
            (
                lang::t(&lang, "Favorites"),
                lang::t(&lang, "Everything you starred, in one place.").to_string(),
            )
        } else if lang::ru(&lang) {
            (
                lang::t(&lang, "Library"),
                format!("{total} звуков · {favorites} избранных"),
            )
        } else {
            (
                lang::t(&lang, "Library"),
                format!("{total} sounds · {favorites} favorites"),
            )
        };
        page_header(ui, title, &subtitle, |ui| {
            if !favorites_only {
                if theme::primary_button(ui, lang::t(&lang, "Add from URL")).clicked() {
                    want_url = true;
                }
                if theme::secondary_button(ui, lang::t(&lang, "Import")).clicked() {
                    want_import = true;
                }
                if theme::secondary_button(ui, lang::t(&lang, "Check files"))
                    .on_hover_text(lang::t(&lang, "Look for sound files that were moved or deleted"))
                    .clicked()
                {
                    want_check = true;
                }
            }
        });
        ui.add_space(12.0);
        theme::inset().show(ui, |ui| {
            ui.horizontal(|ui| {
                let (icon_rect, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
                theme::paint_icon(
                    ui.painter(),
                    Icon::Search,
                    icon_rect.center(),
                    15.0,
                    theme::c_text_faint(),
                );
                ui.add(
                    egui::TextEdit::singleline(&mut self.search)
                        .frame(false)
                        .hint_text(lang::t(&lang, "Search sounds..."))
                        .desired_width(f32::INFINITY),
                );
            });
        });

        // Running downloads.
        let mut cancel: Option<u64> = None;
        let mut ids: Vec<u64> = self.downloads.keys().copied().collect();
        ids.sort_unstable();
        for id in ids {
            let Some(dl) = self.downloads.get(&id) else { continue };
            theme::frame_card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    let text = if lang::ru(&lang) {
                        format!("Скачивается {}", dl.label)
                    } else {
                        format!("Downloading {}", dl.label)
                    };
                    ui.label(RichText::new(text).color(theme::c_text()));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::secondary_button(ui, lang::t(&lang, "Cancel")).clicked() {
                            cancel = Some(id);
                        }
                        const MIB: f64 = 1024.0 * 1024.0;
                        let text = match dl.total {
                            Some(total) if total > 0 => {
                                format!("{:.1} / {:.1} MiB", dl.received as f64 / MIB, total as f64 / MIB)
                            }
                            _ => format!("{:.1} MiB", dl.received as f64 / MIB),
                        };
                        ui.label(RichText::new(text).color(theme::c_text_dim()));
                    });
                });
            });
        }
        if let Some(id) = cancel {
            if let Some(dl) = self.downloads.get(&id) {
                dl.handle.cancel();
            }
        }

        // Downloads that failed stay visible until dismissed (a toast is easy to miss).
        let mut retry: Option<usize> = None;
        let mut dismiss: Option<usize> = None;
        for (i, failed) in self.failed_downloads.iter().enumerate() {
            theme::frame_card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        let text = if lang::ru(&lang) {
                            format!("Не удалось скачать: {}", failed.label)
                        } else {
                            format!("Download failed: {}", failed.label)
                        };
                        ui.label(RichText::new(text).strong().color(theme::c_danger()));
                        ui.label(RichText::new(&failed.error).size(12.5).color(theme::c_text_dim()));
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if theme::secondary_button(ui, lang::t(&lang, "Dismiss")).clicked() {
                            dismiss = Some(i);
                        }
                        if theme::primary_button(ui, lang::t(&lang, "Retry")).clicked() {
                            retry = Some(i);
                        }
                    });
                });
            });
        }
        if let Some(i) = dismiss {
            self.failed_downloads.remove(i);
        } else if let Some(i) = retry {
            let failed = self.failed_downloads.remove(i);
            if let Err(e) = self.start_download(failed.url, failed.play_after) {
                self.toast(ToastKind::Error, lang::tx(&lang, &e));
            }
        }

        // Rows.
        let query = self.search.clone();
        let rows: Vec<Row> = self
            .library
            .search(&query)
            .into_iter()
            .filter(|e| !favorites_only || e.favorite)
            .map(|e| {
                let mut parts: Vec<String> = Vec::new();
                parts.push(e.duration_secs.map_or("--:--".to_string(), theme::fmt_time));
                parts.push(match e.source {
                    ImportSource::Local => lang::t(&lang, "Local file").to_string(),
                    ImportSource::Url(_) => lang::t(&lang, "From URL").to_string(),
                });
                let missing = self.missing.contains(&e.id);
                if (e.volume - 1.0).abs() > 0.01 {
                    let text = if lang::ru(&lang) {
                        format!("Громкость {}%", (e.volume * 100.0).round() as i32)
                    } else {
                        format!("Volume {}%", (e.volume * 100.0).round() as i32)
                    };
                    parts.push(text);
                }
                if missing {
                    parts.push(lang::t(&lang, "File missing").to_string());
                }
                let video = e.path.extension().and_then(|ext| ext.to_str()).is_some_and(|ext| {
                    let ext = ext.to_ascii_lowercase();
                    matches!(ext.as_str(), "mp4" | "m4a" | "mov")
                });
                Row {
                    id: e.id.clone(),
                    name: e.name.clone(),
                    meta: parts.join("  |  "),
                    hotkey: e.hotkey.clone(),
                    hotkey_inactive: self.hotkeys.is_none() || self.inactive_hotkeys.contains(&e.id),
                    favorite: e.favorite,
                    playing: self.engine.is_playing(&e.id),
                    missing,
                    volume: e.volume,
                    video,
                }
            })
            .collect();

        let mut action: Option<Action> = None;
        if rows.is_empty() {
            ui.add_space(44.0);
            theme::frame_card().show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| {
                    let (title, hint) = if !query.trim().is_empty() {
                        (
                            lang::t(&lang, "No sounds match your search"),
                            lang::t(&lang, "Try a different name."),
                        )
                    } else if favorites_only {
                        (
                            lang::t(&lang, "No favorites yet"),
                            lang::t(&lang, "Click the star on a sound to pin it here."),
                        )
                    } else {
                        (
                            lang::t(&lang, "Your library is empty"),
                            lang::t(
                                &lang,
                                "Use Import, drop WAV/MP3/MP4 files here, or add a sound from a URL.",
                            ),
                        )
                    };
                    ui.label(RichText::new(title).size(22.0).strong().color(theme::c_text()));
                    ui.label(RichText::new(hint).color(theme::c_text_dim()));
                });
            });
        } else {
            ui.push_id(self.page as u8, |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add_sized(
                            vec2(30.0, 20.0),
                            egui::Label::new(RichText::new("#").size(12.0).strong().color(theme::c_text_faint())),
                        );
                        ui.label(
                            RichText::new(lang::t(&lang, "Name"))
                                .size(12.0)
                                .strong()
                                .color(theme::c_text_faint()),
                        );
                    });
                    ui.separator();
                    ui.spacing_mut().item_spacing.y = 6.0;
                    for (i, row) in rows.iter().enumerate() {
                        Self::draw_row(ui, i + 1, row, &mut action, &lang);
                    }
                });
            });
        }
        if want_import {
            self.pick_and_import();
        }
        if want_check {
            self.check_library_files();
        }
        if want_url {
            self.url_dialog = Some(UrlDialog {
                text: String::new(),
                error: None,
                focus: true,
            });
        }
        if let Some(a) = action {
            self.apply_action(a);
        }
    }

    fn draw_row(ui: &mut Ui, number: usize, row: &Row, action: &mut Option<Action>, lang: &str) {
        theme::sound_card(row.playing).show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.set_min_height(38.0);
                if row.playing {
                    let (bar, _) = ui.allocate_exact_size(vec2(3.0, 32.0), Sense::hover());
                    ui.painter().rect_filled(bar, Rounding::same(2.0), theme::c_accent());
                }
                ui.add_sized(
                    vec2(28.0, 20.0),
                    egui::Label::new(
                        RichText::new(number.to_string())
                            .size(12.5)
                            .color(theme::c_text_faint()),
                    ),
                );
                let icon = if row.playing { Icon::Stop } else { Icon::Play };
                if theme::round_button(ui, icon, 32.0, true, !row.missing).clicked() {
                    *action = Some(if row.playing {
                        Action::Stop(row.id.clone())
                    } else {
                        Action::Play(row.id.clone())
                    });
                }
                let name_w = (ui.available_width() - 290.0).max(120.0);
                ui.allocate_ui_with_layout(vec2(name_w, 38.0), Layout::top_down(Align::Min), |ui| {
                    ui.set_width(name_w);
                    ui.spacing_mut().item_spacing.y = 1.0;
                    let name_color = if row.playing {
                        theme::c_accent()
                    } else {
                        theme::c_text()
                    };
                    ui.add(
                        egui::Label::new(RichText::new(&row.name).size(15.0).strong().color(name_color))
                            .wrap_mode(egui::TextWrapMode::Truncate),
                    );
                    let meta_color = if row.missing {
                        theme::c_danger()
                    } else {
                        theme::c_text_dim()
                    };
                    ui.add(
                        egui::Label::new(RichText::new(&row.meta).size(12.0).color(meta_color))
                            .wrap_mode(egui::TextWrapMode::Truncate),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    ui.add_space(1.0);
                    ui.menu_button("\u{2022}\u{2022}\u{2022}", |ui| {
                        if ui.button(lang::t(lang, "Rename")).clicked() {
                            *action = Some(Action::Rename(row.id.clone()));
                            ui.close_menu();
                        }
                        if ui.button(lang::t(lang, "Set hotkey...")).clicked() {
                            *action = Some(Action::EditHotkey(row.id.clone()));
                            ui.close_menu();
                        }
                        if row.hotkey.is_some() && ui.button(lang::t(lang, "Clear hotkey")).clicked() {
                            *action = Some(Action::ClearHotkey(row.id.clone()));
                            ui.close_menu();
                        }
                        if row.missing && ui.button(lang::t(lang, "Locate file...")).clicked() {
                            *action = Some(Action::Relink(row.id.clone()));
                            ui.close_menu();
                        }
                        if row.video && ui.button(lang::t(lang, "Extract audio to WAV")).clicked() {
                            *action = Some(Action::ExtractAudio(row.id.clone()));
                            ui.close_menu();
                        }
                        ui.separator();
                        ui.label(
                            RichText::new(lang::t(lang, "Sound volume"))
                                .size(12.5)
                                .color(theme::c_text_dim()),
                        );
                        let mut volume = row.volume;
                        let slider = egui::Slider::new(&mut volume, 0.0..=MAX_ENTRY_VOLUME)
                            .custom_formatter(|v, _| format!("{:.0}%", v * 100.0));
                        if ui.add(slider).changed() {
                            *action = Some(Action::SetVolume(row.id.clone(), volume));
                        }
                        ui.separator();
                        if ui
                            .button(RichText::new(lang::t(lang, "Remove from library")).color(theme::c_danger()))
                            .clicked()
                        {
                            *action = Some(Action::Remove(row.id.clone()));
                            ui.close_menu();
                        }
                    });

                    let star = if row.favorite { Icon::Star } else { Icon::StarOutline };
                    let tip = if row.favorite {
                        lang::t(lang, "Remove from favorites")
                    } else {
                        lang::t(lang, "Add to favorites")
                    };
                    if theme::icon_button(ui, star, 18.0, row.favorite)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        *action = Some(Action::ToggleFavorite(row.id.clone()));
                    }

                    let (text, color, filled) = match (&row.hotkey, row.hotkey_inactive) {
                        (Some(h), false) => (h.clone(), theme::c_accent(), true),
                        (Some(h), true) => (
                            format!("{h} {}", lang::t(lang, "(inactive)")),
                            theme::c_warning(),
                            false,
                        ),
                        (None, _) => (lang::t(lang, "+ Hotkey").to_string(), theme::c_text_dim(), false),
                    };
                    let tip = if row.hotkey_inactive && row.hotkey.is_some() {
                        lang::t(
                            lang,
                            "This combination could not be registered. Click to choose another.",
                        )
                    } else {
                        lang::t(lang, "Click to set a global hotkey")
                    };
                    if theme::chip(ui, &text, color, filled).on_hover_text(tip).clicked() {
                        *action = Some(Action::EditHotkey(row.id.clone()));
                    }
                });
            });
        });
    }
    fn apply_action(&mut self, action: Action) {
        match action {
            Action::Play(id) => self.play_sound(&id),
            Action::Stop(id) => self.engine.stop(&id),
            Action::ToggleFavorite(id) => {
                if let Err(e) = self.library.toggle_favorite(&id) {
                    self.toast(ToastKind::Error, lang::tx(&self.settings.language, &e.to_string()));
                }
                self.save_library();
            }
            Action::Rename(id) => {
                if let Some(entry) = self.library.get(&id) {
                    let text = entry.name.clone();
                    self.rename = Some(RenameDialog { id, text, focus: true });
                }
            }
            Action::EditHotkey(id) => {
                if let Some(entry) = self.library.get(&id) {
                    let name = entry.name.clone();
                    let text = entry.hotkey.clone().unwrap_or_default();
                    self.hotkey_dialog = Some(HotkeyDialog {
                        id,
                        name,
                        text,
                        recording: false,
                        error: None,
                    });
                }
            }
            Action::ClearHotkey(id) => match self.assign_hotkey(&id, None) {
                Ok(()) => self.toast(ToastKind::Info, lang::t(&self.settings.language, "Hotkey cleared")),
                Err(e) => self.toast(ToastKind::Error, lang::tx(&self.settings.language, &e)),
            },
            Action::Remove(id) => self.remove_sound(&id),
            Action::SetVolume(id, volume) => {
                if let Ok(v) = self.library.set_volume(&id, volume) {
                    self.engine.set_sound_gain(&id, self.settings.default_sound_volume * v);
                    self.library_dirty = true;
                    self.sync_index();
                }
            }
            Action::Relink(id) => self.relink_sound(&id),
            Action::ExtractAudio(id) => self.extract_audio(&id),
        }
    }

    // ---- Presence: Discord Rich Presence --------------------------------------------------------

    /// Push the current playback state to Discord (cheap, runs every frame).
    fn sync_presence(&mut self) {
        let now_name = self
            .now_playing
            .as_deref()
            .and_then(|id| self.library.get(id))
            .map(|e| e.name.clone());
        let playing = self.now_playing.as_deref().is_some_and(|id| self.engine.is_playing(id));
        let progress = self.now_playing.as_deref().and_then(|id| self.engine.progress(id));
        let (details, state, started) = match (&now_name, playing, progress) {
            (Some(name), true, Some((pos, total))) => {
                let state = if total > 0.0 {
                    format!("{} / {}", theme::fmt_time(pos), theme::fmt_time(total))
                } else {
                    theme::fmt_time(pos)
                };
                let started = (unix_now() as i64).saturating_sub(pos.round() as i64);
                (Some(name.clone()), Some(state), Some(started))
            }
            (Some(name), _, _) => (Some(name.clone()), None, None),
            (None, _, _) => (None, None, None),
        };
        self.discord.set(details, state, started);
    }

    // ---- First-run setup wizard -----------------------------------------------------------

    /// The setup wizard shown on the first launch: the user picks what to install
    /// (the free VB-CABLE virtual cable, a desktop shortcut, autostart) and VibeDrop
    /// performs the steps before the main window appears.
    fn draw_setup(&mut self, ctx: &egui::Context) {
        let lang = self.settings.language.clone();
        let mut install = false;
        let mut finish = false;
        let mut open_cable_folder = false;
        egui::CentralPanel::default()
            .frame(
                egui::Frame::none()
                    .fill(theme::c_bg())
                    .inner_margin(egui::Margin::symmetric(40.0, 32.0)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    let (mark, _) = ui.allocate_exact_size(vec2(86.0, 86.0), Sense::hover());
                    theme::brand_mark(ui.painter(), mark);
                    ui.add_space(10.0);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(RichText::new("Vibe").size(34.0).strong().color(theme::c_text()));
                        ui.label(RichText::new("Drop").size(34.0).strong().color(theme::c_accent()));
                    });
                    ui.label(
                        RichText::new(lang::t(&lang, "Setup — soundboard for voice chats"))
                            .size(14.0)
                            .color(theme::c_text_dim()),
                    );
                    ui.add_space(22.0);
                    let mut state = self.setup.clone().unwrap_or_default();
                    theme::frame_card().show(ui, |ui| {
                        ui.set_width(560.0);
                        theme::section_title(ui, lang::t(&lang, "WHAT TO INSTALL"));
                        ui.add_space(6.0);
                        ui.checkbox(
                            &mut state.download_cable,
                            lang::t(&lang, "Download the free VB-CABLE virtual audio cable"),
                        );
                        ui.label(
                            RichText::new(lang::t(
                                &lang,
                                "VB-CABLE is freeware by VB-Audio, downloaded from the official site (vb-audio.com). VibeDrop is an independent project and is not affiliated with VB-Audio; the driver itself is not part of VibeDrop and is never redistributed here.",
                            ))
                            .size(11.5)
                            .color(theme::c_text_faint()),
                        );
                        ui.hyperlink_to(
                            lang::t(&lang, "Open the official VB-CABLE page"),
                            "https://vb-audio.com/Cable/",
                        );
                        ui.add_space(8.0);
                        ui.checkbox(
                            &mut state.desktop_shortcut,
                            lang::t(&lang, "Create a desktop shortcut"),
                        );
                        ui.checkbox(
                            &mut state.autostart,
                            lang::t(&lang, "Start VibeDrop when I log in"),
                        );
                        ui.add_space(10.0);
                        if state.download_cable {
                            if state.cable_done {
                                ui.label(
                                    RichText::new(lang::t(&lang, "VB-CABLE downloaded:"))
                                        .color(theme::c_accent()),
                                );
                                ui.label(
                                    RichText::new(state.cable_status.clone())
                                        .size(11.5)
                                        .color(theme::c_text_faint()),
                                );
                                if theme::secondary_button(ui, lang::t(&lang, "Open the folder")).clicked() {
                                    open_cable_folder = true;
                                }
                            } else {
                                ui.label(RichText::new(lang::t(&lang, "Downloading…")).color(theme::c_text_dim()));
                                let _ = egui::ProgressBar::new(state.cable_progress)
                                    .desired_width(520.0);
                            }
                        }
                    });

                    ui.add_space(18.0);
                    ui.horizontal(|ui| {
                        if theme::secondary_button(ui, lang::t(&lang, "Skip")).clicked() {
                            finish = true;
                        }
                        if state.cable_done {
                            if theme::primary_button(ui, lang::t(&lang, "Start VibeDrop")).clicked() {
                                finish = true;
                            }
                        } else {
                            if theme::primary_button(ui, lang::t(&lang, "Install")).clicked() {
                                install = true;
                            }
                        }
                    });
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new(lang::t(
                            &lang,
                            "VibeDrop is free and open source (Apache-2.0). Your library and settings are stored in your user profile and never leave your computer.",
                        ))
                        .size(11.5)
                        .color(theme::c_text_faint()),
                    );
                    if let Some(setup) = self.setup.as_mut() {
                        setup.download_cable = state.download_cable;
                        setup.desktop_shortcut = state.desktop_shortcut;
                        setup.autostart = state.autostart;
                        setup.cable_progress = state.cable_progress;
                        setup.cable_status = state.cable_status.clone();
                        setup.cable_done = state.cable_done;
                    }
                });
            });

        if install {
            self.run_setup();
        }
        if open_cable_folder {
            let dir = self
                .paths
                .root()
                .parent()
                .map(|p| p.join("Downloads"))
                .unwrap_or_else(|| self.paths.root().to_path_buf());
            let _ = std::process::Command::new("explorer").arg(dir).spawn();
        }
        if finish {
            self.settings.setup_done = true;
            self.settings_dirty = true;
            self.save_settings();
            self.setup = None;
        }
    }

    /// Perform the steps the user selected in the setup wizard.
    fn run_setup(&mut self) {
        let Some(setup) = self.setup.as_mut() else {
            return;
        };
        if setup.busy {
            return;
        }
        setup.busy = true;
        if setup.desktop_shortcut {
            let _ = system::create_desktop_shortcut();
        }
        if setup.autostart {
            let _ = system::set_autostart(true);
        }
        if setup.download_cable && !setup.cable_done {
            let tx = self.helper_tx.clone();
            let dest = self.paths.root().join("VBCABLE_Driver_Pack45.zip");
            std::thread::spawn(move || match download_vbcable_with_progress(&dest, &tx) {
                Ok(()) => {
                    let _ = tx.send(HelperMsg::CableReady(dest.display().to_string()));
                }
                Err(e) => {
                    let _ = tx.send(HelperMsg::Cable(Err(e)));
                }
            });
        } else {
            setup.busy = false;
        }
    }

    // ---- Drawing: history --------------------------------------------------------------------

    fn draw_history_page(&mut self, ui: &mut Ui) {
        let lang = self.settings.language.clone();
        let mut clear = false;
        let mut play: Option<String> = None;
        page_header(
            ui,
            lang::t(&lang, "History"),
            lang::t(&lang, "Sounds played in this session, newest first."),
            |ui| {
                if !self.history.is_empty() && theme::secondary_button(ui, lang::t(&lang, "Clear")).clicked() {
                    clear = true;
                }
            },
        );

        if self.history.is_empty() {
            ui.add_space(60.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(lang::t(&lang, "Nothing played yet"))
                        .size(20.0)
                        .strong()
                        .color(theme::c_text()),
                );
                ui.label(
                    RichText::new(lang::t(&lang, "Sounds you play will show up here.")).color(theme::c_text_dim()),
                );
            });
        } else {
            let now = unix_now();
            ui.push_id("history_scroll", |ui| {
                egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                    ui.spacing_mut().item_spacing.y = 8.0;
                    for item in &self.history {
                        let exists = self.library.get(&item.sound_id).is_some();
                        theme::frame_card().show(ui, |ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.set_min_height(36.0);
                                if theme::round_button(ui, Icon::Play, 32.0, false, exists).clicked() {
                                    play = Some(item.sound_id.clone());
                                }
                                let color = if exists { theme::c_text() } else { theme::c_text_faint() };
                                let name_width = (ui.available_width() - 130.0).max(100.0);
                                ui.allocate_ui_with_layout(
                                    vec2(name_width, 36.0),
                                    Layout::left_to_right(Align::Center),
                                    |ui| {
                                        ui.set_width(name_width);
                                        ui.add(
                                            egui::Label::new(RichText::new(&item.name).size(15.5).color(color))
                                                .wrap_mode(egui::TextWrapMode::Truncate),
                                        );
                                    },
                                );
                                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                    ui.label(
                                        RichText::new(theme::fmt_ago(now, item.at))
                                            .size(12.5)
                                            .color(theme::c_text_dim()),
                                    );
                                });
                            });
                        });
                    }
                    ui.add_space(8.0);
                });
            });
        }

        if clear {
            self.history.clear();
            self.save_history();
        }
        if let Some(id) = play {
            self.play_sound(&id);
        }
    }

    // ---- Drawing: settings -------------------------------------------------------------------

    fn draw_settings_page(&mut self, ui: &mut Ui) {
        let lang = self.settings.language.clone();
        let ru = lang::ru(&lang);
        let before = self.settings.clone();
        let mut refresh_devices = false;
        let mut test: Option<OutputRole> = None;
        let mut test_route = false;
        let mut want_cable = false;
        let mut want_sink = false;
        let mut edit_mic_hotkey = false;
        let mut clear_mic_hotkey = false;
        let devices = self.engine.devices();
        let main_state = self.engine.route_state(OutputRole::Main);
        let mic_state = self.engine.route_state(OutputRole::Secondary);
        let voice_state = self.engine.voice_state();
        // On Linux this also drops the ALSA aliases of the desktop default ("default",
        // "pipewire", ...) and the dmix/dsnoop plumbing: the microphone route must never
        // land on the output the user is listening to (see docs/AUDIO_ROUTING.md).
        let mic_choices = sort_for_mic_route(&filter_route_output_names(&devices.outputs));
        let voice_choices = sort_for_voice_input(&devices.inputs);
        let has_virtual = devices.outputs.iter().any(|n| is_virtual_device(n));
        let mut register = false;
        let mut unregister = false;
        let mut export_backup = false;
        let mut import_backup = false;
        let mut open_data_folder = false;

        page_header(
            ui,
            lang::t(&lang, "Settings"),
            lang::t(&lang, "Audio routing, hotkeys and how VibeDrop behaves."),
            |_ui| {},
        );
        ui.push_id("settings_scroll", |ui| {
            egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 14.0;

                // Audio devices.
                theme::frame_card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    theme::section_title(ui, lang::t(&lang, "AUDIO DEVICES"));
                    ui.label(
                        RichText::new(lang::t(
                            &lang,
                            "VibeDrop plays every sound on up to two routes at the same time: your headphones, and a microphone route that voice chats such as Discord can pick up. Output devices (playback) and input devices (microphones) are different things and are chosen separately.",
                        ))
                        .color(theme::c_text_dim()),
                    );

                    // Headphones / speakers.
                    ui.add_space(6.0);
                    ui.label(RichText::new(lang::t(&lang, "Headphones or speakers")).strong().color(theme::c_text()));
                    ui.label(RichText::new(lang::t(&lang, "What you hear.")).size(12.5).color(theme::c_text_faint()));
                    ui.horizontal(|ui| {
                        device_combo(
                            ui,
                            &lang,
                            "main_output",
                            &mut self.settings.main_output,
                            &devices.outputs,
                            devices.default_output.as_deref(),
                        );
                        if theme::secondary_button(ui, lang::t(&lang, "Test sound")).clicked() {
                            test = Some(OutputRole::Main);
                        }
                    });
                    route_status(ui, &lang, &main_state);

                    // Microphone route.
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(4.0);
                    ui.label(RichText::new(lang::t(&lang, "Microphone route")).strong().color(theme::c_text()));
                    ui.label(
                        RichText::new(lang::t(
                            &lang,
                            "What other people hear. Play into a virtual audio cable (e.g. CABLE Input); voice chats then use its recording side as their microphone.",
                        ))
                        .size(12.5)
                        .color(theme::c_text_faint()),
                    );
                    ui.checkbox(
                        &mut self.settings.secondary_enabled,
                        lang::t(&lang, "Send VibeDrop sounds to the microphone route"),
                    );
                    ui.add_enabled_ui(self.settings.secondary_enabled, |ui| {
                        ui.horizontal(|ui| {
                            mic_device_combo(ui, &lang, "secondary_output", &mut self.settings.secondary_output, &mic_choices);
                            if theme::secondary_button(ui, lang::t(&lang, "Test sound"))
                                .on_hover_text(lang::t(
                                    &lang,
                                    "Plays a short tone on the route. People in your voice chat may hear it.",
                                ))
                                .clicked()
                            {
                                test = Some(OutputRole::Secondary);
                            }
                            let label = if self.route_test_running {
                                lang::t(&lang, "Testing...")
                            } else {
                                lang::t(&lang, "Test microphone route")
                            };
                            if ui
                                .add_enabled(!self.route_test_running, egui::Button::new(label))
                                .on_hover_text(lang::t(
                                    &lang,
                                    "Plays a tone on the route and listens on the matching recording device to confirm that it arrives.",
                                ))
                                .clicked()
                            {
                                test_route = true;
                            }
                        });
                    });
                    if self.settings.secondary_enabled {
                        route_status(ui, &lang, &mic_state);
                    } else {
                        route_status(ui, &lang, &RouteState::Disabled);
                    }
                    if let Some(report) = &self.route_test {
                        let color = match report.level {
                            ReportLevel::Ok => theme::c_accent(),
                            ReportLevel::Warning => theme::c_warning(),
                            ReportLevel::Failure => theme::c_danger(),
                        };
                        ui.label(RichText::new(lang::tx(&lang, &report.message)).size(12.5).color(color));
                    }
                    if self.settings.secondary_enabled && !has_virtual && self.settings.secondary_output.is_none() {
                        let msg = if std::env::consts::OS == "linux" {
                            lang::t(
                                &lang,
                                "No virtual audio device (cable) was found on this PC. Without one sound cannot be sent into the microphone. Create a PipeWire null sink — see \"How to set up the microphone route\" below.",
                            )
                            .to_string()
                        } else {
                            lang::t(
                                &lang,
                                "No virtual audio cable was found on this PC. Windows cannot send sound into the microphone without one. Open \"How to set up the microphone route\" below.",
                            )
                            .to_string()
                        };
                        ui.label(RichText::new(msg).size(12.5).color(theme::c_warning()));
                    } else if self.settings.secondary_enabled {
                        if let Some(name) = &self.settings.secondary_output {
                            if devices.scanned && devices.outputs.contains(name) && !is_virtual_device(name) {
                                ui.label(
                                    RichText::new(lang::t(
                                        &lang,
                                        "This does not look like a virtual audio cable. Sound will only reach a voice chat if something routes this device into a microphone.",
                                    ))
                                    .size(12.5)
                                    .color(theme::c_warning()),
                                );
                            }
                        }
                    }

                    if self.settings.secondary_enabled {
                        if let RouteState::Active { device, .. } = &mic_state {
                            let (text, color) = match find_paired_input(device, &devices.inputs) {
                                Some(input) => {
                                    let text = if ru {
                                        format!("В Discord (или в игре) выберите «{input}» в качестве микрофона.")
                                    } else {
                                        format!("In Discord (or your game) choose \"{input}\" as the microphone.")
                                    };
                                    (text, theme::c_accent())
                                }
                                None => {
                                    let text = if ru {
                                        format!("VibeDrop не нашёл устройство записи «{device}» среди устройств записи. Ищите его парное устройство «Output» в голосовом чате.")
                                    } else {
                                        format!("VibeDrop could not find the recording side of \"{device}\" among the recording devices. Look for its matching \"Output\" device in your voice chat.")
                                    };
                                    (text, theme::c_warning())
                                }
                            };
                            ui.label(RichText::new(text).size(12.5).strong().color(color));
                        }
                    }

                    // The user's own microphone, mixed into the microphone route only.
                    ui.add_space(10.0);
                    ui.separator();
                    ui.add_space(4.0);
                    {
                        ui.label(
                            RichText::new(lang::t(&lang, "Your microphone (an input device, optional)"))
                                .strong()
                                .color(theme::c_text()),
                        );
                        let mic_hint = if std::env::consts::OS == "linux" {
                            lang::t(
                                &lang,
                                "A real microphone or headset microphone. Switch this on if you want your voice and VibeDrop's sounds to reach Discord through the one device you choose there. Your voice goes only into the microphone route, never into your headphones. Leave it off if you already send your voice another way (for example, a PipeWire/PulseAudio loopback).",
                            )
                            .to_string()
                        } else {
                            lang::t(
                                &lang,
                                "A real microphone or headset microphone. Switch this on if you want your voice and VibeDrop's sounds to reach Discord through the one device you choose there. Your voice goes only into the microphone route, never into your headphones. Leave it off if you already send your voice another way (for example Windows \"Listen to this device\").",
                            )
                            .to_string()
                        };
                        ui.label(RichText::new(mic_hint).size(12.5).color(theme::c_text_faint()));
                        let route_on = self.settings.secondary_enabled;
                        ui.add_enabled_ui(route_on, |ui| {
                            ui.checkbox(
                                &mut self.settings.voice_enabled,
                                lang::t(&lang, "Mix my microphone into the microphone route"),
                            );
                        });
                        if !route_on {
                            ui.label(
                                RichText::new(lang::t(&lang, "Turn on the microphone route above first."))
                                    .size(12.5)
                                    .color(theme::c_text_faint()),
                            );
                        }
                        ui.add_enabled_ui(route_on && self.settings.voice_enabled, |ui| {
                            device_combo(
                                ui,
                                &lang,
                                "voice_input",
                                &mut self.settings.voice_input,
                                &voice_choices,
                                devices.default_input.as_deref(),
                            );
                        });
                        if route_on && self.settings.voice_enabled {
                            voice_status(ui, &lang, &voice_state);
                        }
                    }

                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if theme::secondary_button(ui, lang::t(&lang, "Refresh devices")).clicked() {
                            refresh_devices = true;
                        }
                        let info = if !devices.scanned {
                            lang::t(&lang, "Looking for audio devices...").to_string()
                        } else if ru {
                            format!(
                                "Устройств вывода: {} / ввода: {}. Вывод по умолчанию: {}. Ввод по умолчанию: {}.",
                                devices.outputs.len(),
                                devices.inputs.len(),
                                devices.default_output.as_deref().unwrap_or("нет"),
                                devices.default_input.as_deref().unwrap_or("нет"),
                            )
                        } else {
                            format!(
                                "{} output / {} input device(s). Default output: {}. Default input: {}.",
                                devices.outputs.len(),
                                devices.inputs.len(),
                                devices.default_output.as_deref().unwrap_or("none"),
                                devices.default_input.as_deref().unwrap_or("none"),
                            )
                        };
                        ui.add(
                            egui::Label::new(RichText::new(info).size(12.5).color(theme::c_text_faint()))
                                .wrap_mode(egui::TextWrapMode::Truncate),
                        );
                    });
                    ui.label(
                        RichText::new(lang::t(&lang, "The list updates by itself when you plug in or remove a device."))
                            .size(12.0)
                            .color(theme::c_text_faint()),
                    );

                    if devices.scanned {
                        let problems = [
                            explain_devices(&lang, DeviceKind::Output, devices.outputs.len(), devices.output_error.as_deref()),
                            explain_devices(&lang, DeviceKind::Input, devices.inputs.len(), devices.input_error.as_deref()),
                        ];
                        for text in problems.into_iter().flatten() {
                            ui.label(RichText::new(text).size(12.5).color(theme::c_warning()));
                        }
                    }
                    let devices_header = if std::env::consts::OS == "windows" {
                        lang::t(&lang, "All devices Windows reports").to_string()
                    } else {
                        lang::t(&lang, "All devices the system reports").to_string()
                    };
                    egui::CollapsingHeader::new(RichText::new(devices_header).color(theme::c_text()))
                        .id_source("device_lists")
                        .show(ui, |ui| {
                            device_list_section(
                                ui,
                                &lang,
                                "Playback devices (outputs)",
                                &devices.outputs,
                                devices.default_output.as_deref(),
                            );
                            ui.add_space(6.0);
                            device_list_section(
                                ui,
                                &lang,
                                "Recording devices (inputs)",
                                &devices.inputs,
                                devices.default_input.as_deref(),
                            );
                        });

                    egui::CollapsingHeader::new(RichText::new(lang::t(&lang, "How to set up the microphone route")).color(theme::c_text()))
                        .id_source("mic_setup_help")
                        .show(ui, |ui| {
                            for line in mic_setup_steps(&lang) {
                                ui.label(RichText::new(line).size(12.5).color(theme::c_text_dim()));
                            }
                            ui.hyperlink_to(lang::t(&lang, "VB-CABLE (free virtual audio cable)"), "https://vb-audio.com/Cable/");
                        });
                });

                // Virtual output: free driver helper (no driver is bundled in VibeDrop).
                theme::frame_card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    theme::section_title(ui, lang::t(&lang, "VIRTUAL OUTPUT (FREE)"));
                    ui.label(
                        RichText::new(lang::t(
                            &lang,
                            "Windows needs a virtual cable for voice chats; VB-CABLE is freeware from the official site. On Linux, VibeDrop creates a built-in virtual sink with no driver at all.",
                        ))
                        .color(theme::c_text_dim()),
                    );
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        if cfg!(target_os = "windows")
                            && theme::secondary_button(ui, lang::t(&lang, "Download free VB-CABLE")).clicked()
                        {
                            want_cable = true;
                        }
                        if cfg!(target_os = "linux")
                            && theme::secondary_button(ui, lang::t(&lang, "Create built-in virtual sink")).clicked()
                        {
                            want_sink = true;
                        }
                    });
                    if cfg!(target_os = "linux") {
                        ui.label(
                            RichText::new("Create a VibeDrop virtual sink (PipeWire/PulseAudio, no installation). In your voice chat, pick VibeDrop (monitor) as the microphone.")
                                .size(12.5)
                                .color(theme::c_text_faint()),
                        );
                    } else {
                        ui.label(
                            RichText::new("Downloads the official VB-CABLE installer once; run it as administrator a single time. No driver ships inside VibeDrop itself.")
                                .size(12.5)
                                .color(theme::c_text_faint()),
                        );
                    }
                });

                // Appearance: theme, accent, radius, font size and panel visibility.
                theme::frame_card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    theme::section_title(ui, lang::t(&lang, "APPEARANCE"));
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(lang::t(&lang, "Theme")).strong());
                        for (id, name) in [
                            ("dark", "Classic dark"),
                            ("midnight", "Midnight"),
                            ("light", "Light"),
                        ] {
                            ui.selectable_value(
                                &mut self.settings.theme.variant,
                                id.to_string(),
                                lang::t(&lang, name),
                            );
                        }
                    });
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(lang::t(&lang, "Accent colour")).strong());
                        let mut accent = self.settings.theme.accent.clone();
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut accent)
                                .desired_width(110.0)
                                .hint_text("#2E9BE6"),
                        );
                        if resp.changed() {
                            self.settings.theme.accent = accent.trim().to_string();
                            self.settings_dirty = true;
                        }
                        for (name, color) in [
                            ("Blue", Color32::from_rgb(0x2E, 0x9B, 0xE6)),
                            ("Teal", Color32::from_rgb(0x2E, 0xC4, 0xB6)),
                            ("Amber", Color32::from_rgb(0xE6, 0x9B, 0x2E)),
                            ("Green", Color32::from_rgb(0x63, 0xC4, 0x54)),
                            ("Pink", Color32::from_rgb(0xE6, 0x5B, 0x9B)),
                        ] {
                            if ui
                                .selectable_label(
                                    self.settings.theme.accent.is_empty()
                                        && theme::c_accent() == color,
                                    name,
                                )
                                .clicked()
                            {
                                self.settings.theme.accent = format!(
                                    "#{:02X}{:02X}{:02X}",
                                    color.r(),
                                    color.g(),
                                    color.b()
                                );
                                self.settings_dirty = true;
                            }
                        }
                    });
                    ui.add_space(6.0);
                    let mut radius = self.settings.theme.corner_radius;
                    if ui
                        .add(
                            egui::Slider::new(&mut radius, 0.0..=20.0)
                                .text("Corner radius"),
                        )
                        .changed()
                    {
                        self.settings.theme.corner_radius = radius;
                        self.settings_dirty = true;
                    }
                    let mut font = self.settings.theme.font_scale;
                    if ui
                        .add(egui::Slider::new(&mut font, 12.0..=20.0).text("Font size"))
                        .changed()
                    {
                        self.settings.theme.font_scale = font;
                        self.settings_dirty = true;
                    }
                    ui.add_space(4.0);
                    ui.horizontal_wrapped(|ui| {
                        let mut toggle = |ui: &mut egui::Ui, label: &str, value: &mut bool| {
                            if ui.checkbox(value, label).changed() {
                                self.settings_dirty = true;
                            }
                        };
                        toggle(
                            ui,
                            lang::t(&lang, "Show sidebar labels"),
                            &mut self.settings.theme.show_sidebar_labels,
                        );
                        toggle(
                            ui,
                            lang::t(&lang, "Show transport bar"),
                            &mut self.settings.theme.show_transport,
                        );
                        toggle(
                            ui,
                            lang::t(&lang, "Show status bar"),
                            &mut self.settings.theme.show_status_bar,
                        );
                        toggle(
                            ui,
                            lang::t(&lang, "Flash window on play"),
                            &mut self.settings.theme.flash_on_play,
                        );
                        toggle(
                            ui,
                            lang::t(&lang, "Show level meter"),
                            &mut self.settings.theme.show_level_meter,
                        );
                    });
                });

                // Playback behaviour.
                theme::frame_card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    theme::section_title(ui, lang::t(&lang, "PLAYBACK BEHAVIOUR"));
                    let mut toggle = |ui: &mut egui::Ui, label: &str, value: &mut bool| {
                        if ui.checkbox(value, label).changed() {
                            self.settings_dirty = true;
                        }
                    };
                    toggle(
                        ui,
                        lang::t(&lang, "Confirm before removing a sound"),
                        &mut self.settings.playback.confirm_remove,
                    );
                    toggle(
                        ui,
                        lang::t(&lang, "Show durations in the table"),
                        &mut self.settings.playback.show_durations,
                    );
                    toggle(
                        ui,
                        lang::t(&lang, "Stop sounds when the window loses focus"),
                        &mut self.settings.playback.stop_on_focus_loss,
                    );
                    ui.add_space(6.0);
                    let mut offset = self.settings.playback.start_offset_secs;
                    if ui
                        .add(egui::Slider::new(&mut offset, 0.0..=30.0).text("Start offset (s)"))
                        .changed()
                    {
                        self.settings.playback.start_offset_secs = offset;
                        self.settings_dirty = true;
                    }
                    let mut voices = self.settings.playback.max_voices;
                    if ui
                        .add(egui::Slider::new(&mut voices, 1..=32).text("Simultaneous sounds"))
                        .changed()
                    {
                        self.settings.playback.max_voices = voices;
                        self.settings_dirty = true;
                    }
                });

                // Volume and playback.
                theme::frame_card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    theme::section_title(ui, lang::t(&lang, "VOLUME AND PLAYBACK"));
                    let pct = |v: f64, _: std::ops::RangeInclusive<usize>| format!("{:.0}%", v * 100.0);
                    egui::Grid::new("playback_grid").num_columns(2).spacing([24.0, 12.0]).show(ui, |ui| {
                        ui.label(lang::t(&lang, "Master volume"));
                        ui.add(egui::Slider::new(&mut self.settings.master_volume, 0.0..=1.0).custom_formatter(pct));
                        ui.end_row();

                        ui.label(lang::t(&lang, "Headphones volume"));
                        ui.add(egui::Slider::new(&mut self.settings.main_volume, 0.0..=1.0).custom_formatter(pct));
                        ui.end_row();

                        ui.label(lang::t(&lang, "Microphone volume"));
                        ui.horizontal(|ui| {
                            ui.add(egui::Slider::new(&mut self.settings.mic_volume, 0.0..=1.0).custom_formatter(pct));
                            ui.checkbox(&mut self.settings.mic_muted, lang::t(&lang, "Muted"));
                        });
                        ui.end_row();

                        ui.label(lang::t(&lang, "Your voice volume"));
                        ui.add_enabled(self.settings.voice_enabled, egui::Slider::new(&mut self.settings.voice_volume, 0.0..=1.0).custom_formatter(pct));
                        ui.end_row();

                        ui.label(lang::t(&lang, "Default sound volume"));
                        ui.add(egui::Slider::new(&mut self.settings.default_sound_volume, 0.0..=1.0).custom_formatter(pct));
                        ui.end_row();

                        ui.label(lang::t(&lang, "Overlapping sounds"));
                        ui.checkbox(&mut self.settings.allow_overlap, lang::t(&lang, "Allow several sounds at once"));
                        ui.end_row();

                        ui.label(lang::t(&lang, "Pressing a playing sound"));
                        egui::ComboBox::from_id_source("retrigger")
                            .width(260.0)
                            .selected_text(retrigger_label(&lang, self.settings.retrigger))
                            .show_ui(ui, |ui| {
                                for policy in [RetriggerPolicy::Restart, RetriggerPolicy::Ignore, RetriggerPolicy::Overlap] {
                                    ui.selectable_value(&mut self.settings.retrigger, policy, retrigger_label(&lang, policy));
                                }
                            });
                        ui.end_row();
                    });
                    ui.label(
                        RichText::new(lang::t(
                            &lang,
                            "Each sound's own volume is in its \u{2022}\u{2022}\u{2022} menu. Final level = master x route x default x sound. Loud overlaps are limited instead of distorting.",
                        ))
                        .size(12.0)
                        .color(theme::c_text_faint()),
                    );
                });

                // Application behavior.
                theme::frame_card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    theme::section_title(ui, lang::t(&lang, "APPLICATION"));
                    egui::Grid::new("app_grid").num_columns(2).spacing([24.0, 12.0]).show(ui, |ui| {
                        ui.label("Language / Язык");
                        egui::ComboBox::from_id_source("language")
                            .width(300.0)
                            .selected_text(if lang::ru(&lang) { "Русский" } else { "English" })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut self.settings.language, "en".to_string(), "English");
                                ui.selectable_value(&mut self.settings.language, "ru".to_string(), "Русский");
                            });
                        ui.end_row();

                        ui.label(lang::t(&lang, "When the window is closed"));
                        egui::ComboBox::from_id_source("close_behavior")
                            .width(300.0)
                            .selected_text(close_label(&lang, self.settings.close_behavior))
                            .show_ui(ui, |ui| {
                                for b in [CloseBehavior::Exit, CloseBehavior::Minimize] {
                                    ui.selectable_value(&mut self.settings.close_behavior, b, close_label(&lang, b));
                                }
                            });
                        ui.end_row();

                        ui.label(lang::t(&lang, "Importing files"));
                        ui.checkbox(
                            &mut self.settings.copy_on_import,
                            lang::t(&lang, "Copy files into the VibeDrop folder"),
                        );
                        ui.end_row();

                        ui.label(lang::t(&lang, "Removing sounds"));
                        ui.checkbox(
                            &mut self.settings.delete_managed_copy_on_remove,
                            lang::t(&lang, "Also delete VibeDrop's own copy (never your original file)"),
                        );
                        ui.end_row();
                    });
                });

                // soundpad:// and vibedrop:// links.
                theme::frame_card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    theme::section_title(ui, lang::t(&lang, "SOUNDPAD LINKS"));
                    ui.label(
                        RichText::new(lang::t(
                            &lang,
                            "Register VibeDrop as the handler of soundpad:// and vibedrop:// links, so that clicking a \"play in Soundpad\" link on a website downloads the sound and plays it here. Only https links are downloaded.",
                        ))
                        .color(theme::c_text_dim()),
                    );
                    let (status, color) = match &self.protocol_state {
                        ProtocolState::Registered => (lang::t(&lang, "Registered for this VibeDrop").to_string(), theme::c_accent()),
                        ProtocolState::NotRegistered => (lang::t(&lang, "Not registered").to_string(), theme::c_text_dim()),
                        ProtocolState::RegisteredElsewhere(cmd) => {
                            let text = if ru {
                                format!("Зарегистрировано для другой программы: {cmd}")
                            } else {
                                format!("Registered for another program: {cmd}")
                            };
                            (text, theme::c_warning())
                        }
                        ProtocolState::Partial(missing) => {
                            let text = if ru {
                                format!("Зарегистрировано частично. Не хватает: {missing}")
                            } else {
                                format!("Partly registered. Missing: {missing}")
                            };
                            (text, theme::c_warning())
                        }
                    };
                    ui.label(RichText::new(status).color(color));
                    ui.horizontal(|ui| {
                        let label = if self.protocol_state == ProtocolState::Registered {
                            lang::t(&lang, "Register again")
                        } else {
                            lang::t(&lang, "Register soundpad:// and vibedrop://")
                        };
                        if theme::primary_button(ui, label).clicked() {
                            register = true;
                        }
                        if self.protocol_state != ProtocolState::NotRegistered
                            && theme::secondary_button(ui, lang::t(&lang, "Unregister")).clicked()
                        {
                            unregister = true;
                        }
                    });
                });

                // Hotkeys and data.
                theme::frame_card().show(ui, |ui| {
                    ui.set_width(ui.available_width());
                    theme::section_title(ui, lang::t(&lang, "GLOBAL HOTKEYS"));
                    match (&self.hotkey_error, &self.hotkeys) {
                        (Some(e), _) => {
                            ui.label(RichText::new(lang::tx(&lang, e)).color(theme::c_danger()));
                        }
                        (None, Some(hk)) => {
                            let text = if ru {
                                format!(
                                    "Активных горячих клавиш: {}. Они работают, пока VibeDrop запущен, даже в фоне, и снимаются при выходе.",
                                    hk.active_count()
                                )
                            } else {
                                format!(
                                    "{} hotkey(s) active. They work while VibeDrop is running, even in the background, and are released when it exits.",
                                    hk.active_count()
                                )
                            };
                            ui.label(RichText::new(text).color(theme::c_text_dim()));
                        }
                        (None, None) => {
                            ui.label(RichText::new(lang::t(&lang, "Hotkeys are not active.")).color(theme::c_text_dim()));
                        }
                    }
                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.label(lang::t(&lang, "Mute / unmute the microphone route"));
                        let (text, color, filled) = match (&self.settings.mic_mute_hotkey, self.mic_hotkey_inactive) {
                            (Some(h), false) => (h.clone(), theme::c_accent(), true),
                            (Some(h), true) => (format!("{h} {}", lang::t(&lang, "(inactive)")), theme::c_warning(), false),
                            (None, _) => (lang::t(&lang, "+ Hotkey").to_string(), theme::c_text_dim(), false),
                        };
                        if theme::chip(ui, &text, color, filled)
                            .on_hover_text(lang::t(&lang, "Click to set a global hotkey"))
                            .clicked()
                        {
                            edit_mic_hotkey = true;
                        }
                        if self.settings.mic_mute_hotkey.is_some()
                            && theme::secondary_button(ui, lang::t(&lang, "Clear")).clicked()
                        {
                            clear_mic_hotkey = true;
                        }
                    });
                    ui.add_space(4.0);
                    theme::section_title(ui, lang::t(&lang, "DATA FOLDER"));
                    ui.label(RichText::new(self.paths.root().display().to_string()).size(12.5).color(theme::c_text_dim()));
                    ui.label(
                        RichText::new(lang::t(
                            &lang,
                            "Your library, settings and the recently played list are stored here and survive restarts and updates.",
                        ))
                        .size(12.5)
                        .color(theme::c_text_faint()),
                    );
                    ui.horizontal(|ui| {
                        if theme::secondary_button(ui, lang::t(&lang, "Export backup")).clicked() {
                            export_backup = true;
                        }
                        if theme::secondary_button(ui, lang::t(&lang, "Import backup")).clicked() {
                            import_backup = true;
                        }
                        if theme::secondary_button(ui, lang::t(&lang, "Open data folder")).clicked() {
                            open_data_folder = true;
                        }
                    });
                });
                ui.add_space(8.0);
            });
        });

        if refresh_devices {
            self.engine.rescan_devices();
            self.engine.configure(self.device_config());
        }
        if export_backup {
            match self.export_backup() {
                Ok(path) => {
                    let text = if ru {
                        format!("Резервная копия сохранена: {path}")
                    } else {
                        format!("Backup saved: {path}")
                    };
                    self.toast(ToastKind::Success, text);
                }
                Err(e) => self.toast(ToastKind::Error, e),
            }
        }
        if import_backup {
            #[cfg(not(target_os = "android"))]
            {
                let picked = rfd::FileDialog::new()
                    .set_title(lang::t(&lang, "Import backup"))
                    .add_filter("VibeDrop backup", &["json"])
                    .pick_file();
                if let Some(path) = picked {
                    match self.import_backup(&path) {
                        Ok(()) => {
                            let text = lang::t(&lang, "Backup restored. Restart VibeDrop to apply it.");
                            self.toast(ToastKind::Success, text);
                        }
                        Err(e) => self.toast(ToastKind::Error, e),
                    }
                }
            }
            #[cfg(target_os = "android")]
            self.toast(
                ToastKind::Info,
                lang::t(&lang, "File import is not available on this platform yet."),
            );
        }
        if open_data_folder {
            self.open_data_folder();
        }
        if let Some(role) = test {
            self.engine.test_tone(role);
        }
        if test_route {
            self.route_test = None;
            self.route_test_running = true;
            self.engine.test_route();
        }
        if want_cable {
            let tx = self.helper_tx.clone();
            let dest = self.paths.root().join("VBCABLE_Driver_Pack45.zip");
            std::thread::spawn(move || {
                let result = download_vbcable(&dest);
                let _ = tx.send(HelperMsg::Cable(result));
            });
        }
        if want_sink {
            let output = std::process::Command::new("pactl")
                .args([
                    "load-module",
                    "module-null-sink",
                    "sink_name=vibedrop",
                    "sink_properties=device.description=VibeDrop",
                ])
                .output();
            let text = match output {
                Ok(o) if o.status.success() => lang::t(
                    &lang,
                    "Built-in sink created: in your voice chat pick VibeDrop (monitor) as the microphone.",
                )
                .to_string(),
                _ => lang::t(&lang, "Could not create sink: is PipeWire/PulseAudio installed?").to_string(),
            };
            self.toast(
                if text.starts_with("Built-in") {
                    ToastKind::Success
                } else {
                    ToastKind::Error
                },
                text,
            );
        }
        if edit_mic_hotkey {
            let text = self.settings.mic_mute_hotkey.clone().unwrap_or_default();
            self.hotkey_dialog = Some(HotkeyDialog {
                id: MIC_TOGGLE_ID.to_string(),
                name: lang::t(&lang, "Microphone mute").to_string(),
                text,
                recording: false,
                error: None,
            });
        }
        if clear_mic_hotkey {
            match self.assign_mic_hotkey(None) {
                Ok(()) => self.toast(ToastKind::Info, lang::t(&lang, "Hotkey cleared")),
                Err(e) => self.toast(ToastKind::Error, lang::tx(&lang, &e)),
            }
        }
        if register {
            match system::register_protocol() {
                Ok(()) => self.toast(
                    ToastKind::Success,
                    lang::t(&lang, "soundpad:// and vibedrop:// links now open VibeDrop"),
                ),
                Err(e) => self.toast(ToastKind::Error, lang::tx(&lang, &e)),
            }
            self.protocol_state = system::protocol_state();
        }
        if unregister {
            match system::unregister_protocol() {
                Ok(()) => self.toast(ToastKind::Info, lang::t(&lang, "Link registration removed")),
                Err(e) => self.toast(ToastKind::Error, lang::tx(&lang, &e)),
            }
            self.protocol_state = system::protocol_state();
        }
        if self.settings != before {
            self.on_settings_changed(&before);
        }
    }

    // ---- Drawing: dialogs and toasts ---------------------------------------------------------

    fn draw_rename_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.rename.take() else { return };
        let lang = self.settings.language.clone();
        let mut keep = true;
        let mut commit = false;
        egui::Window::new(lang::t(&lang, "Rename sound"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                let response = ui.add(egui::TextEdit::singleline(&mut d.text).desired_width(340.0));
                if d.focus {
                    response.request_focus();
                    d.focus = false;
                }
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                ui.horizontal(|ui| {
                    if theme::primary_button(ui, lang::t(&lang, "Save")).clicked() || enter {
                        commit = true;
                    }
                    if theme::secondary_button(ui, lang::t(&lang, "Cancel")).clicked() {
                        keep = false;
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            keep = false;
        }
        if commit {
            match self.library.rename(&d.id, &d.text) {
                Ok(()) => self.save_library(),
                Err(e) => self.toast(ToastKind::Error, lang::tx(&lang, &e.to_string())),
            }
        } else if keep {
            self.rename = Some(d);
        }
    }

    fn draw_hotkey_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.hotkey_dialog.take() else { return };
        let lang = self.settings.language.clone();
        let ru = lang::ru(&lang);

        if d.recording {
            let captured = ctx.input(|i| {
                i.events.iter().find_map(|ev| match ev {
                    egui::Event::Key {
                        key,
                        pressed: true,
                        modifiers,
                        ..
                    } => Some((*key, *modifiers)),
                    _ => None,
                })
            });
            if let Some((key, mods)) = captured {
                if key == Key::Escape && !mods.any() {
                    d.recording = false;
                } else if let Some(token) = key_token(key) {
                    let mut parts: Vec<String> = Vec::new();
                    if mods.ctrl {
                        parts.push("Ctrl".to_string());
                    }
                    if mods.alt {
                        parts.push("Alt".to_string());
                    }
                    if mods.shift {
                        parts.push("Shift".to_string());
                    }
                    parts.push(token);
                    match normalize_hotkey(&parts.join("+")) {
                        Ok(normalized) => {
                            d.text = normalized;
                            d.recording = false;
                            d.error = None;
                        }
                        Err(e) => d.error = Some(e.to_string()),
                    }
                }
            }
        }

        let mut close = false;
        let mut apply = false;
        let mut clear = false;
        let title = if ru {
            format!("Горячая клавиша для «{}»", d.name)
        } else {
            format!("Hotkey for \"{}\"", d.name)
        };
        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(lang::t(
                        &lang,
                        "Click Record and press the combination you want. It works system-wide.",
                    ))
                    .color(theme::c_text_dim()),
                );
                ui.horizontal(|ui| {
                    let hint = if d.recording {
                        lang::t(&lang, "Press a key combination... (Esc cancels)")
                    } else {
                        "Ctrl+Alt+K"
                    };
                    ui.add_enabled(
                        !d.recording,
                        egui::TextEdit::singleline(&mut d.text)
                            .hint_text(hint)
                            .desired_width(260.0),
                    );
                    let label = if d.recording {
                        lang::t(&lang, "Recording...")
                    } else {
                        lang::t(&lang, "Record")
                    };
                    if theme::secondary_button(ui, label).clicked() {
                        d.recording = !d.recording;
                        d.error = None;
                    }
                });
                if let Some(e) = &d.error {
                    ui.label(RichText::new(lang::tx(&lang, e)).color(theme::c_danger()));
                }
                ui.horizontal(|ui| {
                    if theme::primary_button(ui, lang::t(&lang, "Save")).clicked() {
                        apply = true;
                    }
                    if theme::secondary_button(ui, lang::t(&lang, "Clear hotkey")).clicked() {
                        clear = true;
                    }
                    if theme::secondary_button(ui, lang::t(&lang, "Cancel")).clicked() {
                        close = true;
                    }
                });
            });
        if !d.recording && ctx.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }

        if apply {
            let text = d.text.trim().to_string();
            if text.is_empty() {
                d.error = Some(lang::t(&lang, "Enter or record a key combination first.").to_string());
            } else {
                match self.apply_hotkey(&d.id, Some(&text)) {
                    Ok(()) => {
                        close = true;
                        let toast = if ru {
                            format!("Горячая клавиша задана для «{}»", d.name)
                        } else {
                            format!("Hotkey set for \"{}\"", d.name)
                        };
                        self.toast(ToastKind::Success, toast);
                    }
                    Err(e) => d.error = Some(e),
                }
            }
        }
        if clear {
            match self.apply_hotkey(&d.id, None) {
                Ok(()) => close = true,
                Err(e) => d.error = Some(e),
            }
        }
        if !close {
            self.hotkey_dialog = Some(d);
        }
    }

    fn draw_url_dialog(&mut self, ctx: &egui::Context) {
        let Some(mut d) = self.url_dialog.take() else { return };
        let lang = self.settings.language.clone();
        let mut close = false;
        let mut submit = false;
        egui::Window::new(lang::t(&lang, "Add sound from URL"))
            .collapsible(false)
            .resizable(false)
            .anchor(Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(
                    RichText::new(lang::t(
                        &lang,
                        "Paste a direct https:// link to a WAV or MP3 file, or a soundpad:// link.",
                    ))
                    .color(theme::c_text_dim()),
                );
                let response = ui.add(
                    egui::TextEdit::singleline(&mut d.text)
                        .hint_text("https://example.com/sound.mp3")
                        .desired_width(460.0),
                );
                if d.focus {
                    response.request_focus();
                    d.focus = false;
                }
                let enter = response.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
                if let Some(e) = &d.error {
                    ui.label(RichText::new(lang::tx(&lang, e)).color(theme::c_danger()));
                }
                ui.horizontal(|ui| {
                    if theme::primary_button(ui, lang::t(&lang, "Download")).clicked() || enter {
                        submit = true;
                    }
                    if theme::secondary_button(ui, lang::t(&lang, "Cancel")).clicked() {
                        close = true;
                    }
                });
            });
        if ctx.input(|i| i.key_pressed(Key::Escape)) {
            close = true;
        }
        if submit {
            let outcome = Self::parse_target(&d.text).and_then(|url| self.start_download(url, false));
            let error = outcome.err();
            // Static English errors that this dialog itself produces get translated here.
            if error.as_deref() == Some("Paste a link first") && lang::ru(&lang) {
                d.error = Some("Сначала вставьте ссылку".to_string());
            } else {
                d.error = error;
            }
        }
        if !close {
            self.url_dialog = Some(d);
        }
    }

    fn draw_about_dialog(&mut self, ctx: &egui::Context) {
        if !self.about {
            return;
        }
        let lang = self.settings.language.clone();
        let mut open = true;
        egui::Window::new(lang::t(&lang, "About VibeDrop"))
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.set_width(420.0);
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 0.0;
                        ui.label(RichText::new("Vibe").size(22.0).strong().color(theme::c_text()));
                        ui.label(RichText::new("Drop").size(22.0).strong().color(theme::c_accent()));
                    });
                    ui.label(
                        RichText::new("Free open-source soundboard for voice chats (desktop, Rust).")
                            .color(theme::c_text_dim()),
                    );
                    ui.separator();
                    if lang::ru(&lang) {
                        ui.label(
                            RichText::new(
                                "VibeDrop — независимый проект с открытым исходным кодом (лицензия Apache-2.0), вдохновлённый саундбордами в духе Soundpad. Soundpad — товарный знак соответствующего правообладателя; данный проект не аффилирован с ним.\n\nVB-CABLE — бесплатный виртуальный аудиокабель от VB-Audio; он не входит в комплект поставки VibeDrop и не связан с его разработчиками.",
                            )
                            .size(12.5)
                            .color(theme::c_text_dim()),
                        );
                    } else {
                        ui.label(
                            RichText::new(
                                "VibeDrop is an independent open-source project (Apache-2.0), inspired by soundboards in the spirit of Soundpad. Soundpad is a trademark of its respective owner; this project is not affiliated with it.\n\nVB-CABLE is a free virtual audio cable by VB-Audio; it is not bundled with VibeDrop and is not affiliated with its developers.",
                            )
                            .size(12.5)
                            .color(theme::c_text_dim()),
                        );
                    }
                    ui.hyperlink_to(
                        lang::t(&lang, "Official VB-CABLE page"),
                        "https://vb-audio.com/Cable/",
                    );
                });
            });
        if !open {
            self.about = false;
        }
    }

    fn draw_toasts(&mut self, ctx: &egui::Context) {
        let now = Instant::now();
        self.toasts.retain(|t| t.until > now);
        if self.toasts.is_empty() {
            return;
        }
        egui::Area::new(egui::Id::new("toasts"))
            .anchor(Align2::RIGHT_TOP, vec2(-18.0, 18.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 8.0;
                for toast in &self.toasts {
                    let accent = match toast.kind {
                        ToastKind::Info => theme::c_accent_dim(),
                        ToastKind::Success => theme::c_accent(),
                        ToastKind::Error => theme::c_danger(),
                    };
                    egui::Frame::none()
                        .fill(theme::c_card())
                        .stroke(Stroke::new(1.0_f32, accent))
                        .rounding(10.0)
                        .inner_margin(egui::Margin::symmetric(14.0, 10.0))
                        .show(ui, |ui| {
                            ui.set_max_width(380.0);
                            ui.label(RichText::new(&toast.text).color(theme::c_text()));
                        });
                }
            });
    }
}

// ---- Free helpers ----------------------------------------------------------------------------

const MIC_SETUP_STEPS: &[&str] = &[
    "Windows cannot send a program's sound into the microphone by itself. A virtual audio cable is needed, and VibeDrop",
    "does not install drivers. Install one yourself, for example the free VB-CABLE, VoiceMeeter or Virtual Audio Cable.",
    "1. Install it (restart Windows if the installer asks), then press \"Refresh devices\" here.",
    "2. Pick the playback side (\"CABLE Input\") as the microphone route above and press \"Test microphone route\".",
    "3. In Discord, your game or recorder, choose the recording side (\"CABLE Output\") as the microphone.",
    "   VibeDrop shows the exact name under the route once it is running.",
    "4. To let people hear your voice as well, tick \"Mix my microphone into the microphone route\" and pick your",
    "   real microphone. Do not also switch on Windows \"Listen to this device\" for it: you would be heard twice.",
    "5. In Discord > Voice & Video set Noise Suppression to None and turn off automatic input sensitivity, or short",
    "   sounds may be cut off.",
];

/// The setup help lines of the "How to set up the microphone route" section, in the UI language.
fn mic_setup_steps(lang: &str) -> Vec<&'static str> {
    if std::env::consts::OS == "linux" {
        return if lang::ru(lang) {
            vec![
                "Linux-маршрут строится на средствах вашего звукового сервера (PipeWire), драйверы не нужны.",
                "1. Пакет VibeDrop уже создал виртуальный кабель: сторона воспроизведения «vibedrop_route»",
                "   (в неё VibeDrop играет) и микрофон «VibeDrop Microphone (Virtual)» (его выбирает чат).",
                "   Если кабеля нет — переустановите пакет или создайте вручную:",
                "   pactl load-module module-null-sink sink_name=VibeDrop_Route",
                "2. Нажмите здесь «Обновить устройства», выберите «vibedrop_route» как маршрут микрофона выше",
                "   и нажмите «Проверить маршрут микрофона».",
                "3. В Discord, игре или рекордере выберите «VibeDrop Microphone (Virtual)» в качестве микрофона —",
                "   это сторона записи кабеля, аналог «CABLE Output» у VB-CABLE.",
                "4. Чтобы собеседники слышали и ваш голос, включите «Подмешивать мой микрофон в маршрут микрофона»",
                "   и выберите свой настоящий микрофон.",
                "5. Обычные устройства («default», «pipewire», «sysdefault:…») в списке маршрута скрыты: они ведут",
                "   в тот же выход, который вы слышите сами, и ваш голос звучал бы у вас в наушниках.",
            ]
        } else {
            vec![
                "On Linux the route is built from your audio server's facilities (PipeWire); no drivers.",
                "1. The VibeDrop package has already created the virtual cable: the playback side \"vibedrop_route\"",
                "   (VibeDrop plays into it) and the microphone \"VibeDrop Microphone (Virtual)\" (the chat selects it).",
                "   If the cable is missing, reinstall the package or create it by hand:",
                "   pactl load-module module-null-sink sink_name=VibeDrop_Route",
                "2. Press \"Refresh devices\" here, pick \"vibedrop_route\" as the microphone route above and press",
                "   \"Test microphone route\".",
                "3. In Discord, a game or a recorder pick \"VibeDrop Microphone (Virtual)\" as the microphone —",
                "   that is the cable's recording side, the VB-CABLE \"CABLE Output\" equivalent.",
                "4. To let people hear your voice too, enable \"Mix my microphone into the microphone route\" and pick",
                "   your real microphone.",
                "5. Ordinary devices (\"default\", \"pipewire\", \"sysdefault:...\") are hidden from the route list: they",
                "   lead to the very output you are listening to, so your voice would play in your own headphones.",
            ]
        };
    }
    if lang::ru(lang) {
        vec![
            "Windows сама не умеет отправлять звук программы в микрофон. Нужен виртуальный аудиокабель, а VibeDrop",
            "не устанавливает драйверы. Установите кабель сами, например бесплатный VB-CABLE, VoiceMeeter или Virtual Audio Cable.",
            "1. Установите его (если установщик попросит, перезапустите Windows), затем нажмите здесь «Обновить устройства».",
            "2. Выберите сторону воспроизведения («CABLE Input») как маршрут микрофона выше и нажмите «Проверить маршрут микрофона».",
            "3. В Discord, игре или рекордере выберите сторону записи («CABLE Output») в качестве микрофона.",
            "   Точное название VibeDrop покажет под маршрутом, когда тот заработает.",
            "4. Чтобы собеседники слышали и ваш голос, включите «Подмешивать мой микрофон в маршрут микрофона» и выберите свой",
            "   настоящий микрофон. Не включайте для него Windows-функцию «Прослушивать с этого устройства»: вас будут слышать дважды.",
            "5. В Discord > Голос и видео отключите шумоподавление и автоматическую чувствительность микрофона, иначе короткие",
            "   звуки могут обрезаться.",
        ]
    } else {
        MIC_SETUP_STEPS.to_vec()
    }
}

/// One line for the user that says what a route is doing, in the UI language.
/// Mirrors `RouteState::describe`; English falls back to it so both stay in sync.
fn describe_state(lang: &str, state: &RouteState) -> String {
    if !lang::ru(lang) {
        return state.describe();
    }
    match state {
        RouteState::Disabled => lang::t(lang, "Off").to_string(),
        RouteState::NotSelected => lang::t(lang, "No device selected").to_string(),
        RouteState::Active {
            device,
            fallback_from: None,
            ..
        } => format!("Играет через «{device}»"),
        RouteState::Active {
            device,
            fallback_from: Some(wanted),
            ..
        } => format!("«{wanted}» недоступно, звук играет через «{device}»"),
        RouteState::Missing { wanted } => format!("«{wanted}» не подключено. VibeDrop продолжает искать."),
        RouteState::Lost { device, reason } => {
            format!(
                "«{device}» перестало работать ({}). VibeDrop пытается восстановить связь.",
                lang::tx(lang, reason)
            )
        }
        RouteState::Conflict { device } => format!(
            "«{device}» — это ваш обычный выход или та же звуковая карта, что и у наушников. Выберите виртуальный кабель или другое устройство."
        ),
        RouteState::Failed { reason } => format!("Устройство нельзя использовать: {}", lang::tx(lang, reason)),
        RouteState::Standby { reason } => {
            if reason == "Waiting for the microphone route to start." {
                "Ожидание запуска маршрута микрофона.".to_string()
            } else {
                lang::tx(lang, reason)
            }
        }
    }
}

/// A coloured status line for one route.
fn route_status(ui: &mut Ui, lang: &str, state: &RouteState) {
    let (color, text) = match state {
        RouteState::Active {
            device,
            rate,
            fallback_from: None,
            ..
        } => (
            theme::c_accent(),
            if lang::ru(lang) {
                format!("Играет через «{device}» ({} кГц)", *rate as f32 / 1000.0)
            } else {
                format!("Playing on \"{device}\" ({} kHz)", *rate as f32 / 1000.0)
            },
        ),
        RouteState::Active {
            fallback_from: Some(_), ..
        }
        | RouteState::NotSelected => (theme::c_warning(), describe_state(lang, state)),
        RouteState::Lost { .. }
        | RouteState::Conflict { .. }
        | RouteState::Failed { .. }
        | RouteState::Missing { .. } => (theme::c_danger(), describe_state(lang, state)),
        RouteState::Disabled | RouteState::Standby { .. } => (theme::c_text_faint(), describe_state(lang, state)),
    };
    ui.label(RichText::new(format!("\u{25CF} {text}")).size(12.5).color(color));
}

/// Status line of the optional microphone input (it records, it does not play).
fn voice_status(ui: &mut Ui, lang: &str, state: &RouteState) {
    match state {
        RouteState::Active {
            device, rate, channels, ..
        } => {
            let text = if lang::ru(lang) {
                format!(
                    "\u{25CF} Слушаем «{device}» ({} кГц, {channels} канал(ов)) и подмешиваем его в маршрут микрофона",
                    *rate as f32 / 1000.0
                )
            } else {
                format!(
                    "\u{25CF} Listening to \"{device}\" ({} kHz, {channels} ch) and mixing it into the microphone route",
                    *rate as f32 / 1000.0
                )
            };
            ui.label(RichText::new(text).size(12.5).color(theme::c_accent()));
        }
        other => route_status(ui, lang, other),
    }
}

/// Explains why a device list is unusable or empty, in the UI language.
/// English falls back to `explain_device_list` so both stay in sync.
fn explain_devices(lang: &str, kind: DeviceKind, count: usize, error: Option<&str>) -> Option<String> {
    if !lang::ru(lang) {
        return explain_device_list(kind, count, error);
    }
    let noun = match kind {
        DeviceKind::Output => "вывода",
        DeviceKind::Input => "ввода",
    };
    let windows = std::env::consts::OS == "windows";
    if let Some(e) = error {
        return Some(if windows {
            format!(
                "Windows не вернула список устройств {noun} ({}). Проверьте, что запущена служба Windows Audio и \
                 установлен аудиодрайвер, затем нажмите «Обновить устройства».",
                lang::tx(lang, e)
            )
        } else {
            format!(
                "Система не вернула список устройств {noun} ({}). Проверьте, что запущен звуковой сервер \
                 (PipeWire или PulseAudio), затем нажмите «Обновить устройства».",
                lang::tx(lang, e)
            )
        });
    }
    if count > 0 {
        return None;
    }
    if !windows {
        return Some(match kind {
            DeviceKind::Input => {
                "Система не видит ни одного устройства записи. Подключите микрофон или гарнитуру и проверьте, \
                 что он не выключен и не заглушен в настройках звука (PipeWire/PulseAudio или ALSA), затем \
                 нажмите «Обновить устройства»."
                    .to_string()
            }
            DeviceKind::Output => {
                "Система не видит ни одного устройства воспроизведения. Подключите наушники или колонки и \
                 проверьте, что устройство включено в настройках звука (PipeWire/PulseAudio или ALSA), затем \
                 нажмите «Обновить устройства»."
                    .to_string()
            }
        });
    }
    Some(match kind {
        DeviceKind::Input => {
            "Windows не видит ни одного устройства записи. Подключите микрофон или гарнитуру и проверьте \
             Настройки > Система > Звук > Ввод. В классической панели «Звук» (вкладка «Запись») щёлкните правой кнопкой \
             мыши по пустому месту и включите «Показать отключённые устройства», затем включите свой микрофон."
                .to_string()
        }
        DeviceKind::Output => {
            "Windows не видит ни одного устройства воспроизведения. Подключите наушники или колонки и проверьте \
             Настройки > Система > Звук > Вывод. В классической панели «Звук» (вкладка «Воспроизведение») щёлкните \
             правой кнопкой мыши по пустому месту и включите «Показать отключённые устройства», затем включите \
             своё устройство."
                .to_string()
        }
    })
}

/// Device list of the microphone route: no "system default" (that would play into the speakers),
/// virtual cables are listed first and tagged.
fn mic_device_combo(ui: &mut Ui, lang: &str, id: &str, value: &mut Option<String>, devices: &[String]) {
    let not_selected = lang::t(lang, "Not selected").to_string();
    let selected = match value {
        None => not_selected.clone(),
        Some(name) if devices.contains(name) => name.clone(),
        Some(name) => {
            if lang::ru(lang) {
                format!("{name} (не подключено)")
            } else {
                format!("{name} (not connected)")
            }
        }
    };
    egui::ComboBox::from_id_source(id)
        .width(300.0)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, not_selected.as_str());
            for name in devices {
                let label = device_label(lang, name);
                ui.selectable_value(value, Some(name.clone()), label);
            }
        });
}

/// Download the official freeware VB-CABLE installer zip (vb-audio.com) to `dest`.
/// The driver itself is never bundled inside VibeDrop.
fn download_vbcable(dest: &std::path::Path) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .user_agent("VibeDrop/0.3 (desktop soundboard; free cable helper)")
        .timeout(std::time::Duration::from_secs(60))
        .build()
        .map_err(|e| e.to_string())?;
    let resp = client
        .get("https://download.vb-audio.com/Download_CABLE/VBCABLE_Driver_Pack45.zip")
        .send()
        .map_err(|e| e.to_string())?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status()));
    }
    let bytes = resp.bytes().map_err(|e| e.to_string())?;
    std::fs::write(dest, bytes).map_err(|e| e.to_string())?;
    Ok(())
}

/// Download the official freeware VB-CABLE package, reporting progress to the UI.
fn download_vbcable_with_progress(dest: &std::path::Path, tx: &Sender<HelperMsg>) -> Result<(), String> {
    crate::cable::download(dest, |received, total| {
        let _ = tx.try_send(HelperMsg::CableProgress(received, total));
    })
}

/// Write interleaved stereo f32 samples as a 16-bit PCM WAV file.
fn write_wav16(path: &std::path::Path, samples: &[f32], rate: u32) -> std::io::Result<()> {
    use std::io::Write;
    let data_len = (samples.len() * 2) as u32;
    let mut header = [0u8; 44];
    header[0..4].copy_from_slice(b"RIFF");
    header[4..8].copy_from_slice(&(36 + data_len).to_le_bytes());
    header[8..12].copy_from_slice(b"WAVE");
    header[12..16].copy_from_slice(b"fmt ");
    header[16..20].copy_from_slice(&16u32.to_le_bytes());
    header[20..22].copy_from_slice(&1u16.to_le_bytes()); // PCM
    header[22..24].copy_from_slice(&2u16.to_le_bytes()); // stereo
    header[24..28].copy_from_slice(&rate.to_le_bytes());
    header[28..32].copy_from_slice(&(rate * 4).to_le_bytes()); // byte rate
    header[32..34].copy_from_slice(&4u16.to_le_bytes()); // block align
    header[34..36].copy_from_slice(&16u16.to_le_bytes()); // bits per sample
    header[36..40].copy_from_slice(b"data");
    header[40..44].copy_from_slice(&data_len.to_le_bytes());
    let mut file = std::fs::File::create(path)?;
    file.write_all(&header)?;
    for &sample in samples {
        let value = (sample.clamp(-1.0, 1.0) * 32767.0).round() as i16;
        file.write_all(&value.to_le_bytes())?;
    }
    Ok(())
}

/// Page title, subtitle and right-aligned actions. Every page uses the same header rhythm.
fn page_header(ui: &mut Ui, title: &str, subtitle: &str, actions: impl FnOnce(&mut Ui)) {
    ui.horizontal(|ui| {
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing.y = 1.0;
            ui.label(RichText::new(title).size(34.0).strong().color(theme::c_text()));
            ui.label(RichText::new(subtitle).size(13.0).color(theme::c_text_dim()));
        });
        ui.with_layout(Layout::right_to_left(Align::Center), actions);
    });
}

/// "Label  [slider]  100%" inline; returns true when the value changed.
fn volume_row_inline(ui: &mut Ui, label: &str, value: &mut f32, enabled: bool) -> bool {
    ui.add_sized(
        vec2(52.0, 18.0),
        egui::Label::new(RichText::new(label).size(12.5).color(theme::c_text_dim())),
    );
    ui.spacing_mut().slider_width = ui.spacing().slider_width.min(130.0);
    let changed = ui
        .add_enabled(enabled, egui::Slider::new(value, 0.0..=1.0).show_value(false))
        .changed();
    ui.label(
        RichText::new(format!("{:>3}%", (*value * 100.0).round() as i32))
            .monospace()
            .size(12.0)
            .color(theme::c_text_dim()),
    );
    changed
}

fn retrigger_label(lang: &str, policy: RetriggerPolicy) -> &'static str {
    match policy {
        RetriggerPolicy::Restart => lang::t(lang, "Restart the sound"),
        RetriggerPolicy::Ignore => lang::t(lang, "Ignore the new press"),
        RetriggerPolicy::Overlap => lang::t(lang, "Play on top (overlap)"),
    }
}

fn close_label(lang: &str, behavior: CloseBehavior) -> &'static str {
    match behavior {
        CloseBehavior::Exit => lang::t(lang, "Quit VibeDrop"),
        CloseBehavior::Minimize => lang::t(lang, "Minimize (hotkeys keep working)"),
    }
}

/// Combo box for one kind of device: "System default" (naming the actual default device), then
/// every device Windows reports. A saved device that is not present is shown as "not connected"
/// instead of silently becoming something else.
fn device_combo(
    ui: &mut Ui,
    lang: &str,
    id: &str,
    value: &mut Option<String>,
    devices: &[String],
    default_name: Option<&str>,
) {
    let default_label = match default_name {
        Some(d) if lang::ru(lang) => format!("По умолчанию ({d})"),
        Some(d) => format!("System default ({d})"),
        None if lang::ru(lang) => "По умолчанию (устройств нет)".to_string(),
        None => "System default (none available)".to_string(),
    };
    let selected = match value {
        None => default_label.clone(),
        Some(name) if devices.contains(name) => name.clone(),
        Some(name) => {
            if lang::ru(lang) {
                format!("{name} (не подключено)")
            } else {
                format!("{name} (not connected)")
            }
        }
    };
    egui::ComboBox::from_id_source(id)
        .width(300.0)
        .selected_text(selected)
        .show_ui(ui, |ui| {
            ui.selectable_value(value, None, default_label.as_str());
            for name in devices {
                let label = device_label(lang, name);
                ui.selectable_value(value, Some(name.clone()), label);
            }
        });
}

/// A device name with a "(virtual)" tag for virtual audio cables.
fn device_label(lang: &str, name: &str) -> String {
    if is_virtual_device(name) && lang::ru(lang) {
        format!("{name}  (виртуальный)")
    } else if is_virtual_device(name) {
        format!("{name}  (virtual)")
    } else {
        name.to_string()
    }
}

/// A titled list of device names with "default" / "virtual" tags (for the diagnostics section).
fn device_list_section(ui: &mut Ui, lang: &str, title: &'static str, names: &[String], default_name: Option<&str>) {
    ui.label(
        RichText::new(format!("{}: {}", lang::t(lang, title), names.len()))
            .strong()
            .color(theme::c_text_dim()),
    );
    if names.is_empty() {
        ui.label(
            RichText::new(lang::t(lang, "none"))
                .size(12.5)
                .color(theme::c_text_faint()),
        );
    }
    for name in names {
        let mut tags: Vec<&str> = Vec::new();
        if default_name == Some(name.as_str()) {
            tags.push(lang::t(lang, "default"));
        }
        if is_virtual_device(name) {
            tags.push(lang::t(lang, "virtual"));
        }
        let text = if tags.is_empty() {
            name.clone()
        } else {
            format!("{name}  ({})", tags.join(", "))
        };
        ui.label(RichText::new(text).size(12.5).color(theme::c_text_dim()));
    }
}

/// Name of an egui key in the notation understood by `library::normalize_hotkey`.
fn key_token(key: Key) -> Option<String> {
    let digit = match key {
        Key::Num0 => Some('0'),
        Key::Num1 => Some('1'),
        Key::Num2 => Some('2'),
        Key::Num3 => Some('3'),
        Key::Num4 => Some('4'),
        Key::Num5 => Some('5'),
        Key::Num6 => Some('6'),
        Key::Num7 => Some('7'),
        Key::Num8 => Some('8'),
        Key::Num9 => Some('9'),
        _ => None,
    };
    if let Some(d) = digit {
        return Some(d.to_string());
    }
    let special = match key {
        Key::Space => "Space",
        Key::Escape => "Escape",
        Key::Enter => "Enter",
        Key::Tab => "Tab",
        Key::Backspace => "Backspace",
        Key::Delete => "Delete",
        Key::Insert => "Insert",
        Key::Home => "Home",
        Key::End => "End",
        Key::PageUp => "PageUp",
        Key::PageDown => "PageDown",
        Key::ArrowUp => "Up",
        Key::ArrowDown => "Down",
        Key::ArrowLeft => "Left",
        Key::ArrowRight => "Right",
        _ => "",
    };
    if !special.is_empty() {
        return Some(special.to_string());
    }
    let name = key.name();
    let is_letter = name.len() == 1 && name.chars().all(|c| c.is_ascii_alphabetic());
    let is_fkey = name.len() > 1 && name.starts_with('F') && name[1..].chars().all(|c| c.is_ascii_digit());
    if is_letter || is_fkey {
        Some(name.to_string())
    } else {
        None
    }
}

impl eframe::App for VibeDropApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.pump_events(ctx);
        self.handle_close(ctx);
        self.sync_presence();
        // Live appearance changes: re-apply the palette, radius and font size.
        if self.applied_theme != self.settings.theme {
            self.applied_theme = self.settings.theme.clone();
            theme::apply_settings(ctx, &self.settings.theme);
        }

        if self.setup.is_some() {
            self.draw_setup(ctx);
            return;
        }

        self.draw_menubar(ctx);
        self.draw_transport(ctx);
        self.draw_sidebar(ctx);
        self.draw_bottombar(ctx);
        self.draw_central(ctx);

        self.draw_rename_dialog(ctx);
        self.draw_hotkey_dialog(ctx);
        self.draw_url_dialog(ctx);
        self.draw_about_dialog(ctx);
        self.draw_toasts(ctx);
        self.handle_dropped_files(ctx);

        // Write settings once the user stops dragging a slider.
        if self.settings_dirty && !ctx.input(|i| i.pointer.any_down()) {
            self.save_settings();
        }

        // Keep the progress bar moving and the event queues drained, but stay idle otherwise.
        let busy = self.engine.voice_count() > 0 || !self.downloads.is_empty() || !self.toasts.is_empty();
        ctx.request_repaint_after(Duration::from_millis(if busy { 33 } else { 250 }));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.shutdown();
    }
}

impl Drop for VibeDropApp {
    fn drop(&mut self) {
        self.shutdown();
    }
}
