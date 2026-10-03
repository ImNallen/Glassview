use crate::{
    input::{self, Gate},
    overlays::{self, Surface},
    pipeline::{Config, Msg, PipelineHandle},
    preference_saves::PreferenceSaves,
    preferences::Preferences,
    session::{Action, KeyAccess, Session},
    state::{publish, report, snapshot, AppState},
    tray, Result,
};
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

pub(crate) const REPOSITORY_URL: &str = "https://github.com/ImNallen/Glassview";
/// Fixed in v1. Shown as ⌥⇧⌘K on macOS and Ctrl+Alt+Shift+K on Windows.
pub(crate) const TOGGLE_SHORTCUT: &str = "CommandOrControl+Alt+Shift+K";

#[tauri::command]
pub(crate) fn get_session(app: tauri::AppHandle) -> Session {
    snapshot(&app)
}
#[tauri::command]
pub(crate) fn action(app: tauri::AppHandle, action: Action) -> Result<()> {
    perform(&app, action)
}
/// Parsed here rather than by Tauri, which would prefix the messages Settings shows.
#[tauri::command]
pub(crate) fn set_preferences(app: tauri::AppHandle, preferences: serde_json::Value) -> Result<()> {
    let preferences = Preferences::parse(preferences)?;
    {
        let state = app.state::<AppState>();
        let mut session = state.0.lock().unwrap();
        if session.preferences == preferences {
            return Ok(());
        }
        session.preferences = preferences.clone();
    }
    app.state::<PreferenceSaves>().queue(preferences)?;
    apply(&app)
}

pub(crate) fn perform(app: &tauri::AppHandle, action: Action) -> Result<()> {
    match action {
        Action::RequestKeyAccess => return input::request_key_access(app),
        Action::Relaunch => app.restart(),
        Action::Quit => {
            app.exit(0);
            return Ok(());
        }
        _ => {}
    }
    let settings = Surface::Settings
        .window(app)
        .ok_or("Settings window is unavailable")?;
    if action == Action::OpenSettings {
        overlays::center_settings(app, &settings)?;
    }
    let save = app.state::<AppState>().0.lock().unwrap().transition(action);
    if save {
        app.state::<PreferenceSaves>()
            .queue(snapshot(app).preferences)?;
    }
    apply(app)?;
    if action == Action::OpenSettings {
        settings.show()?;
        settings.set_focus()?;
    }
    Ok(())
}

/// Projects the session onto the overlays, the input gate, the pipeline, Settings,
/// and the tray. Idempotent, and the only writer of `Gate`.
pub(crate) fn apply(app: &tauri::AppHandle) -> Result<()> {
    let session = snapshot(app);
    let Preferences { keys, halo, .. } = session.preferences;
    overlays::set_overlays_visible(app, session.enabled)?;
    app.state::<Arc<Gate>>().set(session.enabled, keys, halo);
    app.state::<PipelineHandle>().send(Msg::Config(Config {
        enabled: session.enabled,
        halo,
    }));
    if !session.settings_open {
        if let Some(settings) = Surface::Settings.window(app) {
            settings.hide()?;
        }
    }
    tray::sync_tray(app, &session)?;
    publish(app)
}

#[cfg_attr(not(target_os = "macos"), allow(dead_code))]
pub(crate) fn set_key_access(app: &tauri::AppHandle, access: KeyAccess) -> Result<()> {
    app.state::<AppState>()
        .0
        .lock()
        .unwrap()
        .set_key_access(access);
    apply(app)
}

pub(crate) fn register_toggle(app: &tauri::AppHandle) -> Result<()> {
    app.global_shortcut()
        .on_shortcut(TOGGLE_SHORTCUT, |app, _, event| {
            if event.state() == ShortcutState::Pressed {
                if let Err(error) = perform(app, Action::Toggle) {
                    report(app, error);
                }
            }
        })
        .map_err(|e| format!("Could not register {}: {e}", snapshot(app).shortcut).into())
}
