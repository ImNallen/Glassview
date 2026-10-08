use crate::{
    input::{self, Gate},
    overlays::{self, Surface},
    pipeline::{Config, Msg, PipelineHandle},
    preference_saves::PreferenceSaves,
    preferences::Preferences,
    session::{Action, Session},
    state::{publish, report, snapshot, AppState},
    tray, Result,
};
use std::sync::Arc;
use tauri::Manager;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_opener::OpenerExt;

pub(crate) const REPOSITORY_URL: &str = "https://github.com/ImNallen/Glassview";
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
        Action::OpenGithub => {
            app.opener().open_url(REPOSITORY_URL, None::<&str>)?;
            return Ok(());
        }
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
        overlays::anchor_settings(app, &settings)?;
    }
    let save = app.state::<AppState>().0.lock().unwrap().transition(action);
    if save {
        app.state::<PreferenceSaves>()
            .queue(snapshot(app).preferences)?;
    }
    apply(app)?;
    if action == Action::OpenSettings {
        // A menu-bar app is never frontmost on its own, so Settings would open behind
        // the active app and not become key, which keeps Escape from reaching it.
        #[cfg(target_os = "macos")]
        app.run_on_main_thread(activate_app)?;
        settings.show()?;
        settings.set_focus()?;
    }
    Ok(())
}

/// `activate` needs macOS 14; this supports 12.
#[cfg(target_os = "macos")]
pub(crate) fn activate_app() {
    use objc2_app_kit::NSApplication;
    let Some(mtm) = objc2::MainThreadMarker::new() else {
        return;
    };
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
}

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

#[cfg(target_os = "macos")]
pub(crate) fn set_key_access(
    app: &tauri::AppHandle,
    access: crate::session::KeyAccess,
) -> Result<()> {
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
