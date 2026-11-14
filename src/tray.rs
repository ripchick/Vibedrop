//! The tray icon: closing the window hides VibeDrop in the tray instead of quitting it.
//!
//! The icon is the same picture the program uses everywhere else (the PNG in `assets/`), so
//! there is only one icon to maintain.

use std::sync::mpsc::{channel, Receiver, Sender};

/// The program icon, decoded into raw pixels for the tray.
const ICON_PNG: &[u8] = include_bytes!("../assets/icon.png");

/// What the user chose in the tray menu.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayAction {
    /// Show the window again.
    Show,
    /// Quit the program.
    Quit,
}

/// The tray icon with its menu.
pub struct Tray {
    actions: Receiver<TrayAction>,
    /// Kept alive: dropping the icon would remove it from the tray.
    #[allow(dead_code)]
    icon: tray_icon::TrayIcon,
}

impl Tray {
    /// Create the tray icon. Returns `None` when the system does not allow one (for example on
    /// a headless machine).
    pub fn new() -> Option<Tray> {
        let (tx, actions) = channel();
        let menu = muda::Menu::new();
        let show = muda::MenuItem::new("Show VibeDrop", true, None);
        let quit = muda::MenuItem::new("Quit VibeDrop", true, None);
        menu.append(&show).ok()?;
        menu.append(&quit).ok()?;
        let icon = app_icon()?;
        let tray = tray_icon::TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            .with_icon(icon)
            .with_tooltip("VibeDrop")
            .with_menu_on_left_click(false)
            .build()
            .ok()?;
        let show_id = show.id().clone();
        let quit_id = quit.id().clone();
        let sender: Sender<TrayAction> = tx;
        std::thread::spawn(move || {
            let receiver = muda::MenuEvent::receiver();
            while let Ok(event) = receiver.recv() {
                let id = event.id();
                let action = if id == &show_id {
                    Some(TrayAction::Show)
                } else if id == &quit_id {
                    Some(TrayAction::Quit)
                } else {
                    None
                };
                if let Some(action) = action {
                    if sender.send(action).is_err() {
                        break;
                    }
                }
            }
        });
        Some(Tray { actions, icon: tray })
    }

    /// The action the user chose, if any.
    pub fn action(&self) -> Option<TrayAction> {
        self.actions.try_recv().ok()
    }
}

/// The program icon as raw pixels.
fn app_icon() -> Option<tray_icon::Icon> {
    let image = image::load_from_memory(ICON_PNG).ok()?;
    let rgba = image.to_rgba8();
    let (width, height) = rgba.dimensions();
    tray_icon::Icon::from_rgba(rgba.into_raw(), width, height).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_icon_can_be_decoded() {
        // The tray icon is optional, but the picture itself has to be readable.
        let image = image::load_from_memory(ICON_PNG).expect("the icon is a readable picture");
        assert!(image.width() > 0 && image.height() > 0);
    }
}
