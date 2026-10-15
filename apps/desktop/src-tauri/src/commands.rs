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
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};
use tauri_plugin_opener::OpenerExt;

pub(crate) const REPOSITORY_URL: &str = "https://github.com/ImNallen/Glassview";
pub(crate) const TOGGLE_SHORTCUT: &str = "CommandOrControl+Alt+Shift+K";

#[tauri::command]
pub(crate) fn get_session(app: tauri::AppHandle) -> Session {
    snapshot(&app)
}
/// Async so `perform` runs off the main thread. Creating a webview from a synchronous
/// command deadlocks on Windows (`WebviewWindowBuilder::new` docs, wry#583).
#[tauri::command]
pub(crate) async fn action(app: tauri::AppHandle, action: Action) -> Result<()> {
    perform(&app, action)
}
/// The settings page calls this once it has rendered the session, so a new window
/// never shows default values.
#[tauri::command]
pub(crate) fn settings_ready(app: tauri::AppHandle, webview: tauri::WebviewWindow) -> Result<()> {
    if Surface::parse(webview.label()) == Some(Surface::Settings) && snapshot(&app).settings_open {
        present_settings(&app, &webview)?;
    }
    Ok(())
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
        Action::ToggleLaunchAtLogin => {
            let autolaunch = app.autolaunch();
            if snapshot(app).launch_at_login {
                autolaunch.disable()
            } else {
                autolaunch.enable()
            }
            .map_err(|e| format!("Could not change launch at login: {e}"))?;
            refresh_launch_at_login(app);
            return apply(app);
        }
        Action::OpenSettings => refresh_launch_at_login(app),
        Action::Relaunch => app.restart(),
        Action::Quit => {
            app.exit(0);
            return Ok(());
        }
        _ => {}
    }
    let save = app.state::<AppState>().0.lock().unwrap().transition(action);
    if save {
        app.state::<PreferenceSaves>()
            .queue(snapshot(app).preferences)?;
    }
    apply(app)?;
    if action == Action::OpenSettings {
        // A window `apply` just created stays hidden until its page calls `settings_ready`.
        if let Some(settings) = Surface::Settings.window(app) {
            if matches!(settings.is_visible(), Ok(true)) {
                present_settings(app, &settings)?;
            }
        }
    }
    Ok(())
}

pub(crate) fn refresh_launch_at_login(app: &tauri::AppHandle) {
    match app.autolaunch().is_enabled() {
        Ok(enabled) => app.state::<AppState>().0.lock().unwrap().launch_at_login = enabled,
        Err(error) => log::warn!("Could not read launch at login: {error}"),
    }
}

fn present_settings(app: &tauri::AppHandle, settings: &tauri::WebviewWindow) -> Result<()> {
    overlays::anchor_settings(app, settings)?;
    // A menu-bar app is never frontmost on its own, so Settings would open behind
    // the active app and not become key, which keeps Escape from reaching it.
    #[cfg(target_os = "macos")]
    app.run_on_main_thread(activate_app)?;
    settings.show()?;
    Ok(settings.set_focus()?)
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
    let Preferences {
        keys, halo, clicks, ..
    } = session.preferences;
    overlays::set_overlays_visible(app, session.enabled)?;
    app.state::<Arc<Gate>>().set(session.enabled, keys, halo);
    app.state::<PipelineHandle>().send(Msg::Config(Config {
        enabled: session.enabled,
        halo,
        clicks,
    }));
    // Settings exists only while open; its webview process would otherwise stay resident.
    match Surface::Settings.window(app) {
        None if session.settings_open => overlays::create_settings(app)?,
        Some(settings) if !session.settings_open => settings.destroy()?,
        _ => {}
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
