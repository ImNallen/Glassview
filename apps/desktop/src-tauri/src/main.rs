#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod error;
mod input;
mod keys;
mod layout;
mod overlays;
mod pipeline;
mod preference_saves;
mod preferences;
mod session;
mod startup_error;
mod state;
mod tray;

use commands::{action, get_session, perform, register_toggle, set_preferences};
use error::Result;
use input::{Gate, Sink};
use overlays::Surface;
use pipeline::{Config, Pipeline};
use preference_saves::PreferenceSaves;
use preferences::Preferences;
use session::{Action, Session};
use state::{report, snapshot, AppState};
use std::sync::{Arc, Mutex};
use tauri::Manager;

fn main() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!("{info}\n{}", std::backtrace::Backtrace::force_capture());
        default_hook(info);
    }));
    let app = tauri::Builder::default()
        // Registered first so a second launch exits before creating windows,
        // a tray icon, input hooks, or a competing global shortcut.
        .plugin(tauri_plugin_single_instance::init(|app, _, _| {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || open_settings(&handle));
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .max_file_size(1_000_000)
                .rotation_strategy(tauri_plugin_log::RotationStrategy::KeepSome(3))
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                if Surface::parse(window.label()) == Some(Surface::Settings) {
                    if let Err(error) = perform(window.app_handle(), Action::CloseSettings) {
                        report(window.app_handle(), error);
                    }
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            get_session,
            action,
            set_preferences
        ])
        .setup(|app| {
            if let Err(error) = setup(app) {
                // Tauri panics on a setup error, which closes the app silently.
                startup_error::exit(&error.to_string());
            }
            Ok(())
        })
        .build(tauri::generate_context!());
    let app = app.unwrap_or_else(|error| startup_error::exit(&error.to_string()));
    app.run(|app, event| {
        if matches!(
            event,
            tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit
        ) {
            if let Err(error) = app.state::<PreferenceSaves>().flush() {
                log::error!("Could not save preferences on exit: {error}");
            }
        }
        #[cfg(target_os = "macos")]
        if let tauri::RunEvent::Reopen { .. } = event {
            open_settings(app);
        }
    });
}

fn setup(app: &mut tauri::App) -> std::result::Result<(), Box<dyn std::error::Error>> {
    log::info!(
        "Starting Glassview {} on {} {}",
        app.package_info().version,
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let mut problems = Vec::new();
    let handle = app.handle().clone();
    let (preferences, warning) = Preferences::load(&handle);
    problems.extend(warning);
    let key_access = input::key_access();
    app.manage(AppState(Mutex::new(Session::new(preferences, key_access))));
    app.manage(PreferenceSaves::start(handle.clone()));
    #[cfg(target_os = "macos")]
    app.set_activation_policy(tauri::ActivationPolicy::Accessory);
    let layout = overlays::current_layout(&handle).map_err(|e| e.to_string())?;
    let session = snapshot(&handle);
    let gate = Arc::new(Gate::default());
    let config = Config {
        enabled: session.enabled,
        halo: session.preferences.halo,
    };
    let pipeline = pipeline::start(
        handle.clone(),
        Pipeline::new(keys::Os::CURRENT, config, layout.clone()),
    );
    app.manage(gate.clone());
    app.manage(pipeline.clone());
    overlays::create_settings(app)?;
    tray::create_tray(app)?;
    problems.extend(
        overlays::sync(&handle, &layout)
            .err()
            .map(|e| e.to_string()),
    );
    problems.extend(
        input::start(&handle, Sink::new(gate, pipeline))
            .err()
            .map(|e| e.to_string()),
    );
    if let Err(error) = register_toggle(&handle) {
        log::error!("{error}");
        app.state::<AppState>()
            .0
            .lock()
            .unwrap()
            .shortcut_unavailable = true;
    }
    problems.extend(commands::apply(&handle).err().map(|e| e.to_string()));
    let session = snapshot(&handle);
    if !problems.is_empty() || session.shortcut_unavailable || !session.preferences.onboarded {
        open_settings(&handle);
    }
    if !problems.is_empty() {
        report(&handle, problems.join(" "));
    }
    overlays::watch_displays(handle, layout);
    Ok(())
}

fn open_settings(app: &tauri::AppHandle) {
    if let Err(error) = perform(app, Action::OpenSettings) {
        report(app, error);
    }
}
