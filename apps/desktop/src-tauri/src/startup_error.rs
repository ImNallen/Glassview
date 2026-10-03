use crate::commands::REPOSITORY_URL;

const TITLE: &str = "Glassview could not start";

pub(crate) fn exit(error: &str) -> ! {
    log::error!("Could not start Glassview: {error}");
    show_native(&format!(
        "{error}\n\nTry opening Glassview again. If this keeps happening, \
         report it at {REPOSITORY_URL}/issues and include Glassview's log file."
    ));
    std::process::exit(1);
}

#[cfg(target_os = "macos")]
fn show_native(details: &str) {
    use objc2::MainThreadMarker;
    use objc2_app_kit::{NSAlert, NSAlertStyle, NSApplication};
    use objc2_foundation::NSString;
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let alert = NSAlert::new(mtm);
    alert.setAlertStyle(NSAlertStyle::Critical);
    alert.setMessageText(&NSString::from_str(TITLE));
    alert.setInformativeText(&NSString::from_str(details));
    // A menu-bar app is not activated on launch, so the alert would open
    // behind the frontmost app. `activate` needs macOS 14; this supports 12.
    #[allow(deprecated)]
    NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
    alert.runModal();
}

#[cfg(target_os = "windows")]
fn show_native(details: &str) {
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        MessageBoxW, MB_ICONERROR, MB_OK, MB_SETFOREGROUND,
    };
    let wide = |text: &str| text.encode_utf16().chain([0]).collect::<Vec<u16>>();
    let (text, caption) = (wide(details), wide(TITLE));
    unsafe {
        MessageBoxW(
            std::ptr::null_mut(),
            text.as_ptr(),
            caption.as_ptr(),
            MB_OK | MB_ICONERROR | MB_SETFOREGROUND,
        );
    }
}

#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn show_native(_details: &str) {}
