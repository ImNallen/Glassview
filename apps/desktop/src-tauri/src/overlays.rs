use crate::{
    commands, input,
    layout::{InputSpace, Layout, MonitorInfo, Point, Rect},
    pipeline::{Msg, PipelineHandle},
    state::report,
    Result,
};
use std::time::Duration;
use tauri::{Manager, WebviewUrl, WebviewWindowBuilder};

const SETTINGS_CSS_SIZE: (f64, f64) = (380.0, 600.0);

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub(crate) enum Surface {
    Overlay(usize),
    Settings,
}
impl Surface {
    pub(crate) fn url(self) -> &'static str {
        match self {
            Self::Overlay(_) => "overlay",
            Self::Settings => "settings",
        }
    }
    pub(crate) fn label(self) -> String {
        match self {
            Self::Overlay(index) => format!("overlay-{index}"),
            Self::Settings => "settings".into(),
        }
    }
    pub(crate) fn parse(label: &str) -> Option<Self> {
        match label.strip_prefix("overlay-") {
            Some(index) => index.parse().ok().map(Self::Overlay),
            None => (label == "settings").then_some(Self::Settings),
        }
    }
    pub(crate) fn window<R: tauri::Runtime>(
        self,
        app: &impl Manager<R>,
    ) -> Option<tauri::WebviewWindow<R>> {
        app.get_webview_window(&self.label())
    }
}

pub(crate) fn current_layout(app: &tauri::AppHandle) -> Result<Layout> {
    let monitors: Vec<_> = app
        .available_monitors()?
        .iter()
        .map(MonitorInfo::from)
        .collect();
    Ok(Layout::from_monitors(&monitors, InputSpace::CURRENT))
}

pub(crate) fn create_settings(app: &tauri::App) -> tauri::Result<()> {
    WebviewWindowBuilder::new(
        app,
        Surface::Settings.label(),
        WebviewUrl::App(format!("index.html?surface={}", Surface::Settings.url()).into()),
    )
    .title("Glassview Settings")
    .inner_size(SETTINGS_CSS_SIZE.0, SETTINGS_CSS_SIZE.1)
    .resizable(false)
    .maximizable(false)
    .minimizable(false)
    .visible(false)
    .build()?;
    Ok(())
}

pub(crate) fn sync(app: &tauri::AppHandle, layout: &Layout) -> Result<()> {
    for (index, display) in layout.displays().iter().enumerate() {
        let window = match Surface::Overlay(index).window(app) {
            Some(window) => window,
            None => build_overlay(app, index)?,
        };
        place(&window, display.bounds)?;
    }
    for (label, window) in app.webview_windows() {
        if let Some(Surface::Overlay(index)) = Surface::parse(&label) {
            if index >= layout.displays().len() {
                window.destroy()?;
            }
        }
    }
    log::info!(
        "Overlays cover {} display(s): {:?}",
        layout.displays().len(),
        layout.displays()
    );
    app.state::<PipelineHandle>()
        .send(Msg::Layout(layout.clone()));
    commands::apply(app)
}

fn build_overlay(app: &tauri::AppHandle, index: usize) -> Result<tauri::WebviewWindow> {
    let window = WebviewWindowBuilder::new(
        app,
        Surface::Overlay(index).label(),
        WebviewUrl::App(format!("index.html?surface={}", Surface::Overlay(index).url()).into()),
    )
    .title("Glassview overlay")
    .transparent(true)
    .decorations(false)
    .shadow(false)
    .always_on_top(true)
    .skip_taskbar(true)
    .resizable(false)
    .focusable(false)
    .focused(false)
    .visible(false)
    .build()?;
    window.set_ignore_cursor_events(true)?;
    let native = window.clone();
    window.run_on_main_thread(move || configure_native(&native))?;
    Ok(window)
}

fn configure_native(window: &tauri::WebviewWindow) {
    #[cfg(target_os = "macos")]
    if let Ok(ptr) = window.ns_window() {
        use objc2_app_kit::{
            NSScreenSaverWindowLevel, NSWindow, NSWindowCollectionBehavior as Behavior,
        };
        let native = unsafe { &*(ptr as *const NSWindow) };
        native.setLevel(NSScreenSaverWindowLevel);
        native.setCollectionBehavior(
            Behavior::CanJoinAllSpaces
                | Behavior::FullScreenAuxiliary
                | Behavior::Stationary
                | Behavior::IgnoresCycle,
        );
    }
    #[cfg(target_os = "windows")]
    if let Ok(hwnd) = window.hwnd() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            GetWindowLongPtrW, SetWindowLongPtrW, GWL_EXSTYLE, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW,
        };
        let hwnd = hwnd.0 as _;
        unsafe {
            let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
            SetWindowLongPtrW(
                hwnd,
                GWL_EXSTYLE,
                style | (WS_EX_NOACTIVATE | WS_EX_TOOLWINDOW) as isize,
            );
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = window;
}

fn place(window: &tauri::WebviewWindow, bounds: Rect) -> Result<()> {
    #[cfg(target_os = "macos")]
    {
        use tauri::{LogicalPosition, LogicalSize};
        // Resize first: macOS keeps the bottom edge when resizing, so a later resize would shift the top.
        window.set_size(LogicalSize::new(bounds.width, bounds.height))?;
        window.set_position(LogicalPosition::new(bounds.x, bounds.y))?;
    }
    #[cfg(not(target_os = "macos"))]
    {
        use tauri::{PhysicalPosition, PhysicalSize};
        let (position, size) = (
            PhysicalPosition::new(bounds.x as i32, bounds.y as i32),
            PhysicalSize::new(bounds.width as u32, bounds.height as u32),
        );
        // Moving onto a display with another scale resizes the window (WM_DPICHANGED),
        // so check where it landed and apply both again once if needed.
        for attempt in 0..2 {
            window.set_position(position)?;
            window.set_size(size)?;
            if window.outer_position()? == position && window.outer_size()? == size {
                return Ok(());
            }
            if attempt == 1 {
                log::warn!(
                    "Overlay {} landed at {:?} {:?} instead of {position:?} {size:?}",
                    window.label(),
                    window.outer_position()?,
                    window.outer_size()?
                );
            }
        }
    }
    Ok(())
}

/// Shows overlays without activating them, since Tauri's `show` would make them key
/// on macOS. Hiding is native too: tao on Windows tracks visibility itself and would
/// treat a window it never showed as already hidden.
pub(crate) fn set_overlays_visible(app: &tauri::AppHandle, visible: bool) -> Result<()> {
    for (label, window) in app.webview_windows() {
        if matches!(Surface::parse(&label), Some(Surface::Overlay(_))) {
            let native = window.clone();
            window.run_on_main_thread(move || set_native_visible(&native, visible))?;
        }
    }
    Ok(())
}

fn set_native_visible(window: &tauri::WebviewWindow, visible: bool) {
    #[cfg(target_os = "macos")]
    if let Ok(ptr) = window.ns_window() {
        let native = unsafe { &*(ptr as *const objc2_app_kit::NSWindow) };
        if visible {
            native.orderFrontRegardless();
        } else {
            native.orderOut(None);
        }
    }
    #[cfg(target_os = "windows")]
    if let Ok(hwnd) = window.hwnd() {
        use windows_sys::Win32::UI::WindowsAndMessaging::{
            SetWindowPos, ShowWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE,
            SW_HIDE, SW_SHOWNOACTIVATE,
        };
        let hwnd = hwnd.0 as _;
        if !visible {
            unsafe { ShowWindow(hwnd, SW_HIDE) };
            return;
        }
        unsafe {
            ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            SetWindowPos(
                hwnd,
                HWND_TOPMOST,
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            );
        }
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (window, visible);
}

pub(crate) fn center_settings(app: &tauri::AppHandle, window: &tauri::WebviewWindow) -> Result<()> {
    let layout = current_layout(app)?;
    let display = input::cursor()
        .and_then(|cursor| layout.locate(cursor))
        .and_then(|(index, _)| layout.displays().get(index))
        .or(layout.displays().first())
        .ok_or("No display is available for settings")?;
    let (bounds, k) = (display.bounds, display.css_per_unit);
    let at = Point {
        x: bounds.x + (bounds.width - SETTINGS_CSS_SIZE.0 / k) / 2.0,
        y: bounds.y + (bounds.height - SETTINGS_CSS_SIZE.1 / k) / 2.0,
    };
    #[cfg(target_os = "macos")]
    window.set_position(tauri::LogicalPosition::new(at.x, at.y))?;
    #[cfg(not(target_os = "macos"))]
    window.set_position(tauri::PhysicalPosition::new(at.x as i32, at.y as i32))?;
    Ok(())
}

pub(crate) fn watch_displays(app: tauri::AppHandle, initial: Layout) {
    std::thread::spawn(move || {
        let mut last = initial;
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let layout = match current_layout(&app) {
                Ok(layout) if layout != last => layout,
                Ok(_) => continue,
                Err(error) => {
                    log::warn!("Could not read displays: {error}");
                    continue;
                }
            };
            let (done, synced) = std::sync::mpsc::channel();
            let (handle, target) = (app.clone(), layout.clone());
            let scheduled = app.run_on_main_thread(move || {
                let result = sync(&handle, &target);
                if let Err(error) = &result {
                    report(&handle, error);
                }
                let _ = done.send(result.is_ok());
            });
            if scheduled.is_err() {
                break;
            }
            if synced.recv() == Ok(true) {
                last = layout;
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::Surface::{self, *};
    #[test]
    fn surfaces_keep_the_labels_and_urls_the_frontend_reads() {
        for (surface, label, url) in [
            (Overlay(2), "overlay-2", "overlay"),
            (Settings, "settings", "settings"),
        ] {
            assert_eq!((surface.label().as_str(), surface.url()), (label, url));
            assert_eq!(Surface::parse(label), Some(surface));
        }
        for label in ["overlay-", "overlay-x", "main"] {
            assert_eq!(Surface::parse(label), None);
        }
    }
}
