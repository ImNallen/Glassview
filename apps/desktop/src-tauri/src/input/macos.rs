use super::Sink;
use crate::{
    commands,
    keys::{self, Key, KeyPress, Mods, Named},
    layout::Point,
    pipeline::Button,
    session::KeyAccess,
    state::report,
    Result,
};
use block2::RcBlock;
use objc2::MainThreadMarker;
use objc2_app_kit::{NSEvent, NSEventMask};
use objc2_core_foundation::{kCFRunLoopCommonModes, CFMachPort, CFRetained, CFRunLoop};
use objc2_core_graphics::{
    CGEvent, CGEventField, CGEventFlags, CGEventTapLocation, CGEventTapOptions,
    CGEventTapPlacement, CGEventTapProxy, CGEventType,
};
use std::{
    ffi::c_void,
    panic::{catch_unwind, AssertUnwindSafe},
    ptr::{null, null_mut, NonNull},
    sync::{
        atomic::{AtomicPtr, Ordering},
        OnceLock,
    },
    time::Duration,
};
use tauri_plugin_opener::OpenerExt;

const INPUT_MONITORING_PANE: &str =
    "x-apple.systempreferences:com.apple.preference.security?Privacy_ListenEvent";
const LISTEN_EVENT: u32 = 1;

#[link(name = "IOKit", kind = "framework")]
extern "C" {
    fn IOHIDCheckAccess(request: u32) -> u32;
    fn IOHIDRequestAccess(request: u32) -> bool;
}
// HIToolbox keyboard layouts, which objc2 does not wrap.
#[link(name = "Carbon", kind = "framework")]
extern "C" {
    static kTISPropertyUnicodeKeyLayoutData: *const c_void;
    fn TISCopyCurrentKeyboardLayoutInputSource() -> *const c_void;
    fn TISCopyCurrentASCIICapableKeyboardLayoutInputSource() -> *const c_void;
    fn TISGetInputSourceProperty(source: *const c_void, key: *const c_void) -> *const c_void;
    fn LMGetKbdType() -> u8;
    #[allow(clippy::too_many_arguments)]
    fn UCKeyTranslate(
        layout: *const u8,
        key_code: u16,
        key_action: u16,
        modifier_key_state: u32,
        keyboard_type: u32,
        options: u32,
        dead_key_state: *mut u32,
        max_length: usize,
        actual_length: *mut usize,
        unicode: *mut u16,
    ) -> i32;
}
#[link(name = "CoreFoundation", kind = "framework")]
extern "C" {
    fn CFRelease(cf: *const c_void);
    fn CFDataGetBytePtr(data: *const c_void) -> *const u8;
}

static SINK: OnceLock<Sink> = OnceLock::new();
// Kept for the app's lifetime so a tap the system disables can be re-enabled.
static KEY_TAP: AtomicPtr<CFMachPort> = AtomicPtr::new(null_mut());

pub(super) fn start(app: &tauri::AppHandle, sink: Sink) -> Result<()> {
    if SINK.set(sink).is_err() {
        return Err("Input capture is already running".into());
    }
    install_mouse_monitor()?;
    log::info!("Installed the mouse event monitor");
    match key_access() {
        KeyAccess::Granted => install_key_tap(),
        access => {
            log::info!("Key display is waiting for Input Monitoring ({access:?})");
            watch_key_access(app.clone(), access);
            Ok(())
        }
    }
}

pub(super) fn key_access() -> KeyAccess {
    match unsafe { IOHIDCheckAccess(LISTEN_EVENT) } {
        0 => KeyAccess::Granted,
        1 => KeyAccess::Denied,
        _ => KeyAccess::Unknown,
    }
}

/// The system prompt appears only while access is undecided. After a denial,
/// only System Settings can grant it.
pub(super) fn request_key_access(app: &tauri::AppHandle) -> Result<()> {
    match key_access() {
        KeyAccess::Granted => Ok(()),
        KeyAccess::Unknown => {
            std::thread::spawn(|| unsafe { IOHIDRequestAccess(LISTEN_EVENT) });
            Ok(())
        }
        KeyAccess::Denied => Ok(app.opener().open_url(INPUT_MONITORING_PANE, None::<&str>)?),
    }
}

/// An AppKit global monitor, not an event tap: creating any tap, even a listen-only
/// mouse tap, shows the Input Monitoring prompt and records a denial before the user
/// chooses. A monitor needs no permission and calls back on the main thread.
fn install_mouse_monitor() -> Result<()> {
    let mask = NSEventMask::LeftMouseDown
        | NSEventMask::RightMouseDown
        | NSEventMask::OtherMouseDown
        | NSEventMask::LeftMouseUp
        | NSEventMask::RightMouseUp
        | NSEventMask::OtherMouseUp
        | NSEventMask::MouseMoved
        | NSEventMask::LeftMouseDragged
        | NSEventMask::RightMouseDragged
        | NSEventMask::OtherMouseDragged;
    let handler = RcBlock::new(|event: NonNull<NSEvent>| {
        let _ = catch_unwind(AssertUnwindSafe(|| on_mouse(unsafe { event.as_ref() })));
    });
    let monitor = NSEvent::addGlobalMonitorForEventsMatchingMask_handler(mask, &handler)
        .ok_or("macOS refused to monitor mouse events")?;
    std::mem::forget(monitor);
    Ok(())
}

/// A listen-only session tap whose source joins the main run loop in common modes, so
/// callbacks run on the main thread (UCKeyTranslate requires it) and keep firing
/// during menu tracking. Listen-only taps never delay input.
fn install_key_tap() -> Result<()> {
    let tap = unsafe {
        CGEvent::tap_create(
            CGEventTapLocation::SessionEventTap,
            CGEventTapPlacement::HeadInsertEventTap,
            CGEventTapOptions::ListenOnly,
            1 << CGEventType::KeyDown.0,
            Some(key_callback),
            null_mut(),
        )
    }
    .ok_or("macOS refused to create an event tap")?;
    let source = CFMachPort::new_run_loop_source(None, Some(&tap), 0)
        .ok_or("Could not attach the event tap")?;
    let run_loop = CFRunLoop::main().ok_or("The main run loop is unavailable")?;
    run_loop.add_source(Some(&source), unsafe { kCFRunLoopCommonModes });
    CGEvent::tap_enable(&tap, true);
    KEY_TAP.store(CFRetained::into_raw(tap).as_ptr(), Ordering::Release);
    log::info!("Installed the keyboard event tap");
    Ok(())
}

/// Polls for as long as access is missing, since the grant happens in System Settings
/// at any time.
fn watch_key_access(app: tauri::AppHandle, mut last: KeyAccess) {
    std::thread::spawn(move || loop {
        std::thread::sleep(Duration::from_secs(2));
        let access = key_access();
        if access == last {
            continue;
        }
        last = access;
        let handle = app.clone();
        let scheduled = app.run_on_main_thread(move || {
            if access == KeyAccess::Granted {
                if let Err(error) = install_key_tap() {
                    report(&handle, format!("Could not start key display: {error}"));
                }
            }
            if let Err(error) = commands::set_key_access(&handle, access) {
                report(&handle, error);
            }
        });
        if scheduled.is_err() || access == KeyAccess::Granted {
            break;
        }
    });
}

fn reenable_key_tap() {
    if let Some(tap) = NonNull::new(KEY_TAP.load(Ordering::Acquire)) {
        CGEvent::tap_enable(unsafe { tap.as_ref() }, true);
    }
}

fn location(event: &CGEvent) -> Point {
    let at = CGEvent::location(Some(event));
    Point { x: at.x, y: at.y }
}

fn on_mouse(event: &NSEvent) {
    let Some(sink) = SINK.get().filter(|sink| sink.gate.enabled()) else {
        return;
    };
    // The CGEvent's location is in global top-left points, the space `Layout` uses.
    let Some(event) = event.CGEvent() else {
        return;
    };
    let event = &*event;
    let at = location(event);
    match CGEvent::r#type(Some(event)) {
        kind @ (CGEventType::LeftMouseDown
        | CGEventType::RightMouseDown
        | CGEventType::OtherMouseDown) => {
            if let Some(button) = button(kind, event) {
                sink.press(button, mods(CGEvent::flags(Some(event))), at);
            }
        }
        kind @ (CGEventType::LeftMouseUp
        | CGEventType::RightMouseUp
        | CGEventType::OtherMouseUp) => {
            if let Some(button) = button(kind, event) {
                sink.release(button, at);
            }
        }
        CGEventType::MouseMoved => sink.moved(at, false),
        CGEventType::LeftMouseDragged
        | CGEventType::RightMouseDragged
        | CGEventType::OtherMouseDragged => sink.moved(at, true),
        _ => {}
    }
}

fn button(kind: CGEventType, event: &CGEvent) -> Option<Button> {
    match kind {
        CGEventType::LeftMouseDown | CGEventType::LeftMouseUp => Some(Button::Left),
        CGEventType::RightMouseDown | CGEventType::RightMouseUp => Some(Button::Right),
        _ => Button::from_mac_number(CGEvent::integer_value_field(
            Some(event),
            CGEventField::MouseEventButtonNumber,
        )),
    }
}

fn mods(flags: CGEventFlags) -> Mods {
    [
        (CGEventFlags::MaskControl, Mods::CTRL),
        (CGEventFlags::MaskAlternate, Mods::ALT),
        (CGEventFlags::MaskShift, Mods::SHIFT),
        (CGEventFlags::MaskCommand, Mods::META),
    ]
    .into_iter()
    .filter(|(flag, _)| flags.contains(*flag))
    .fold(Mods::NONE, |mods, (_, m)| mods | m)
}

unsafe extern "C-unwind" fn key_callback(
    _proxy: CGEventTapProxy,
    kind: CGEventType,
    event: NonNull<CGEvent>,
    _user_info: *mut c_void,
) -> *mut CGEvent {
    let _ = catch_unwind(AssertUnwindSafe(|| on_key(kind, unsafe { event.as_ref() })));
    event.as_ptr()
}

fn on_key(kind: CGEventType, event: &CGEvent) {
    if kind != CGEventType::KeyDown {
        return reenable_key_tap();
    }
    let Some(sink) = SINK.get().filter(|sink| sink.gate.enabled()) else {
        return;
    };
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let flags = CGEvent::flags(Some(event));
    let mods = mods(flags);
    let mode = sink.gate.mode();
    if !keys::may_show(mods, mode) {
        return;
    }
    let code = CGEvent::integer_value_field(Some(event), CGEventField::KeyboardEventKeycode) as u16;
    let Some(key) = named_key(code).map(Key::Named).or_else(|| {
        translate(mtm, code, 0)
            .filter(|c| !c.is_control())
            .map(Key::Char)
    }) else {
        return;
    };
    // UCKeyTranslate takes Carbon modifier bits shifted right by 8: shift, caps lock, option.
    let layout_mods = [
        (CGEventFlags::MaskShift, 0x02),
        (CGEventFlags::MaskAlphaShift, 0x04),
        (CGEventFlags::MaskAlternate, 0x08),
    ]
    .into_iter()
    .filter(|(flag, _)| flags.contains(*flag))
    .fold(0, |bits, (_, bit)| bits | bit);
    let text = keys::mac_typed_text(mods, || translate(mtm, code, layout_mods));
    if let Some(stroke) = keys::admit(KeyPress { mods, key, text }, mode) {
        sink.key(stroke, location(event));
    }
}

/// kVK_* virtual key codes for keys with no character.
fn named_key(code: u16) -> Option<Named> {
    Some(match code {
        36 | 76 => Named::Enter,
        48 => Named::Tab,
        49 => Named::Space,
        51 => Named::Backspace,
        117 => Named::Delete,
        53 => Named::Escape,
        114 => Named::Insert,
        123 => Named::Left,
        124 => Named::Right,
        125 => Named::Down,
        126 => Named::Up,
        115 => Named::Home,
        119 => Named::End,
        116 => Named::PageUp,
        121 => Named::PageDown,
        122 => Named::F(1),
        120 => Named::F(2),
        99 => Named::F(3),
        118 => Named::F(4),
        96 => Named::F(5),
        97 => Named::F(6),
        98 => Named::F(7),
        100 => Named::F(8),
        101 => Named::F(9),
        109 => Named::F(10),
        103 => Named::F(11),
        111 => Named::F(12),
        105 => Named::F(13),
        107 => Named::F(14),
        113 => Named::F(15),
        106 => Named::F(16),
        64 => Named::F(17),
        79 => Named::F(18),
        80 => Named::F(19),
        90 => Named::F(20),
        _ => return None,
    })
}

fn translate(_mtm: MainThreadMarker, code: u16, modifiers: u32) -> Option<char> {
    unsafe {
        let mut source = TISCopyCurrentKeyboardLayoutInputSource();
        let mut data = layout_data(source);
        // Input methods such as Japanese Kana have no Unicode layout of their own.
        if data.is_null() {
            release(source);
            source = TISCopyCurrentASCIICapableKeyboardLayoutInputSource();
            data = layout_data(source);
        }
        let typed = (!data.is_null())
            .then(|| key_translate(CFDataGetBytePtr(data), code, modifiers))
            .flatten();
        release(source);
        typed
    }
}

unsafe fn layout_data(source: *const c_void) -> *const c_void {
    if source.is_null() {
        return null();
    }
    unsafe { TISGetInputSourceProperty(source, kTISPropertyUnicodeKeyLayoutData) }
}

unsafe fn release(source: *const c_void) {
    if !source.is_null() {
        unsafe { CFRelease(source) };
    }
}

unsafe fn key_translate(layout: *const u8, code: u16, modifiers: u32) -> Option<char> {
    const KEY_DOWN: u16 = 0;
    // A local dead-key state leaves the user's own pending accent untouched.
    let mut dead_key_state = 0;
    let mut length = 0;
    let mut unicode = [0u16; 4];
    let status = unsafe {
        UCKeyTranslate(
            layout,
            code,
            KEY_DOWN,
            modifiers,
            u32::from(LMGetKbdType()),
            0,
            &mut dead_key_state,
            unicode.len(),
            &mut length,
            unicode.as_mut_ptr(),
        )
    };
    if status != 0 || dead_key_state != 0 {
        return None;
    }
    let mut chars = char::decode_utf16(unicode[..length].iter().copied());
    match (chars.next(), chars.next()) {
        (Some(Ok(c)), None) => Some(c),
        _ => None,
    }
}
