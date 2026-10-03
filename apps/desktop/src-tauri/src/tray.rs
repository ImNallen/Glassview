use crate::{
    commands::{perform, TOGGLE_SHORTCUT},
    layout::Rect,
    session::{Action, KeyAccess, Session},
    state::report,
    Result,
};
use std::sync::Mutex;
use tauri::{
    image::Image,
    menu::{CheckMenuItem, Menu, MenuItem, PredefinedMenuItem},
    Manager,
};

const TRAY_ID: &str = "glassview-tray";

pub(crate) struct Tray {
    menu: Menu<tauri::Wry>,
    toggle: CheckMenuItem<tauri::Wry>,
    request_key_access: MenuItem<tauri::Wry>,
    shown: Mutex<Shown>,
}
#[derive(Default)]
struct Shown {
    enabled: Option<bool>,
    request_key_access: bool,
}

fn icon(enabled: bool) -> Image<'static> {
    // macOS colors a template's alpha mask for the current menu bar appearance.
    #[cfg(target_os = "macos")]
    let (on, off) = (
        tauri::include_image!("icons/tray-on-template.png"),
        tauri::include_image!("icons/tray-off-template.png"),
    );
    #[cfg(not(target_os = "macos"))]
    let (on, off) = (
        tauri::include_image!("icons/tray-on.png"),
        tauri::include_image!("icons/tray-off.png"),
    );
    if enabled {
        on
    } else {
        off
    }
}

fn menu_id(action: Action) -> String {
    match serde_json::to_value(action) {
        Ok(serde_json::Value::String(id)) => id,
        _ => unreachable!("{action:?} serializes to a string"),
    }
}

pub(crate) fn create_tray(app: &tauri::App) -> tauri::Result<()> {
    let toggle = CheckMenuItem::with_id(
        app,
        menu_id(Action::Toggle),
        "Show clicks and keys",
        true,
        true,
        Some(TOGGLE_SHORTCUT),
    )?;
    let request_key_access = MenuItem::with_id(
        app,
        menu_id(Action::RequestKeyAccess),
        "Allow key display…",
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(
        app,
        menu_id(Action::Quit),
        "Quit Glassview",
        true,
        None::<&str>,
    )?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(app, &[&toggle, &separator, &quit])?;
    tauri::tray::TrayIconBuilder::with_id(TRAY_ID)
        .icon(icon(true))
        .icon_as_template(cfg!(target_os = "macos"))
        .tooltip("Glassview")
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_tray_icon_event(|tray, event| {
            use tauri::tray::{MouseButton, MouseButtonState, TrayIconEvent};
            if matches!(
                event,
                TrayIconEvent::Click {
                    button: MouseButton::Left,
                    button_state: MouseButtonState::Up,
                    ..
                }
            ) {
                if let Err(error) = perform(tray.app_handle(), Action::OpenSettings) {
                    report(tray.app_handle(), error);
                }
            }
        })
        .on_menu_event(|app, event| {
            let action = serde_json::from_value(event.id.as_ref().into());
            if let Err(e) = action
                .map_err(Into::into)
                .and_then(|action| perform(app, action))
            {
                report(app, e);
            }
        })
        .build(app)?;
    app.manage(Tray {
        menu,
        toggle,
        request_key_access,
        shown: Mutex::default(),
    });
    Ok(())
}

pub(crate) fn sync_tray(app: &tauri::AppHandle, session: &Session) -> Result<()> {
    let (Some(state), Some(tray)) = (app.try_state::<Tray>(), app.tray_by_id(TRAY_ID)) else {
        return Ok(());
    };
    // Clicking a check item flips it natively, so set it every time.
    state.toggle.set_checked(session.enabled)?;
    let mut shown = state.shown.lock().unwrap();
    if shown.enabled != Some(session.enabled) {
        tray.set_icon(Some(icon(session.enabled)))?;
        #[cfg(target_os = "macos")]
        tray.set_icon_as_template(true)?;
        tray.set_tooltip(Some(if session.enabled {
            "Glassview"
        } else {
            "Glassview (off)"
        }))?;
        shown.enabled = Some(session.enabled);
    }
    let request = cfg!(target_os = "macos") && session.key_access != KeyAccess::Granted;
    if request != shown.request_key_access {
        if request {
            state.menu.insert(&state.request_key_access, 1)?;
        } else {
            state.menu.remove(&state.request_key_access)?;
        }
        shown.request_key_access = request;
    }
    Ok(())
}

pub(crate) fn bounds(app: &tauri::AppHandle) -> Result<Rect> {
    let tray = app.tray_by_id(TRAY_ID).ok_or("Tray icon is unavailable")?;
    #[cfg(target_os = "macos")]
    let scale = tray
        .with_inner_tray_icon(|tray| {
            let mtm = objc2::MainThreadMarker::new()?;
            tray.ns_status_item()?
                .button(mtm)?
                .window()
                .map(|window| window.backingScaleFactor())
        })?
        .ok_or("Tray display is unavailable")?;
    #[cfg(not(target_os = "macos"))]
    let scale = 1.0;
    let rect = tray.rect()?.ok_or("Tray position is unavailable")?;
    let position = rect.position.to_physical::<f64>(scale);
    let size = rect.size.to_physical::<f64>(scale);
    Ok(Rect {
        x: position.x / scale,
        y: position.y / scale,
        width: size.width / scale,
        height: size.height / scale,
    })
}
